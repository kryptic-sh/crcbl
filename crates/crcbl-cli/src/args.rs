//! Argument parsing, and the decision not to take `clap`.
//!
//! # Decision: hand-rolled, revisited when the CLI reaches its P9 shape
//!
//! `clap` is the obvious choice, it is well maintained, its licence passes
//! `deny.toml`, and it is not a framework that would own engine policy. It was
//! still rejected for P0, on four grounds:
//!
//! 1. **Size, against this workspace's baseline.** The whole workspace has five
//!    third-party dependencies (`bitflags`, `glam`, `log`, `proptest`,
//!    `thiserror`). `clap` with its default features brings ten to twelve
//!    crates. It would be, by a wide margin, the largest dependency in the
//!    engine — to parse three subcommands and nine flags.
//! 2. **Consistency with decisions already made.** `docs/plan/15-windowing.md`
//!    rejected `winit`, `x11rb` and `wayland-client` and this workspace
//!    hand-wrote a Wayland protocol scanner and its own libxcb bindings rather
//!    than take them. A CLI that reaches for a parser generator while the
//!    window system is hand-rolled is not a coherent position.
//! 3. **The policy is ours anyway.** The exit-code contract (0 ok, 1 failed, 2
//!    bad invocation), the `--json` mirror of every human message, and "no
//!    prompt unless a TTY" are all decisions this crate makes and tests. `clap`
//!    would supply the tokenizer, which is the part that is one `match`.
//! 4. **What it buys is not needed yet.** Derive macros, subcommand trees,
//!    shell completions and colourised help earn their keep at ten subcommands
//!    with interlocking flags. At three, they are unused surface.
//!
//! **The reconsideration point is explicit**: topic 11 schedules `scene`,
//! `import`, `screenshot`, `sim`, `phys` and `edit` for P1–P12, several with
//! stdin batch modes and nested flags. When this file passes roughly two
//! hundred lines of `match`, or the first subcommand needs a value that is
//! neither a string nor a count, take `clap` and delete this module. The parser
//! is deliberately isolated behind [`Invocation`] so that swap touches one
//! file.
//!
//! `cargo-machete` and `cargo-deny` both gate CI, and a crate with no
//! dependencies passes both trivially — which is a nice property, not the
//! argument.

use std::path::PathBuf;

/// Top-level `--help`.
pub const USAGE: &str = "\
crcbl — the Crucible engine's headless control CLI

USAGE:
    crcbl <COMMAND> [OPTIONS]

COMMANDS:
    new <NAME>    Scaffold a game project that builds and runs immediately.
    run           Run the project in the current directory.
    build         Build the project in the current directory.
    screenshot    Offscreen render the scene and write a PNG.
    replay        Read a .crpl replay file and dump its metadata.

OPTIONS (every command):
        --json    Emit one JSON object instead of human output.
    -h, --help    Print help for the command, or this text.
    -V, --version Print the version.

EXIT CODES:
    0  ok        1  the command failed        2  bad invocation

Run `crcbl <COMMAND> --help` for a command's own options.";

/// `crcbl new --help`.
pub const NEW_USAGE: &str = "\
crcbl new — scaffold a game project

USAGE:
    crcbl new <NAME> [OPTIONS]

The result is a standalone Cargo project with its own workspace, depending on
the engine by path. It builds and runs the moment it is created:

    crcbl new mygame && cd mygame && crcbl run --headless

OPTIONS:
        --path <DIR>     Create the project under DIR instead of the current
                         directory.
        --engine <DIR>   Use this Crucible checkout instead of searching
                         upwards from the current directory for one.
        --force          Overwrite an existing, non-empty directory.
        --json           Emit one JSON object instead of human output.
    -h, --help           Print this text.";

/// `crcbl run --help`.
pub const RUN_USAGE: &str = "\
crcbl run — run the project in the current directory

USAGE:
    crcbl run [OPTIONS] [-- <ARGS>...]

Everything after `--` is passed to the game unchanged.

OPTIONS:
        --headless          Run with no window. Passed through to the game,
                            which is what implements it.
    -p, --package <NAME>    Build and run this package (for a workspace).
        --release           Build with optimizations.
        --json              Emit one JSON object instead of human output.
    -h, --help              Print this text.";

/// `crcbl build --help`.
pub const BUILD_USAGE: &str = "\
crcbl build — build the project in the current directory

USAGE:
    crcbl build [OPTIONS]

OPTIONS:
        --target <TARGET>   `native` (the default). `wasm` is planned for P5
                            and fails cleanly until then.
    -p, --package <NAME>    Build this package (for a workspace).
        --release           Build with optimizations.
        --json              Emit one JSON object instead of human output.
    -h, --help              Print this text.";

/// `crcbl screenshot --help`.
pub const SCREENSHOT_USAGE: &str = "\
crcbl screenshot — offscreen render to PNG

USAGE:
    crcbl screenshot [OPTIONS]

Renders one frame using the auto-selected GPU backend through an offscreen
swapchain, reads the pixels back, and writes a PNG.

OPTIONS:
        --size WxH       Output dimensions. Default: 1920x1080.
    -o, --output <FILE>  Write the PNG here. Default: screenshot.png.
        --json           Emit one JSON object instead of human output.
    -h, --help           Print this text.";

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invocation {
    /// A command to carry out.
    Command(Command),
    /// Print this text and exit 0.
    Help(&'static str),
    /// Print the version and exit 0.
    Version,
    /// The invocation is malformed, for this reason. Exit 2.
    BadUsage(String),
}

/// One of the four subcommands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Scaffold a project.
    New(NewArgs),
    /// Run one.
    Run(RunArgs),
    /// Build one.
    Build(BuildArgs),
    /// Offscreen render → PNG.
    Screenshot(ScreenshotArgs),
    /// Read a .crpl replay file.
    Replay(ReplayArgs),
}

impl Command {
    /// The name that appears in `--json` output and in error messages.
    pub fn name(&self) -> &'static str {
        match self {
            Self::New(_) => "new",
            Self::Run(_) => "run",
            Self::Build(_) => "build",
            Self::Screenshot(_) => "screenshot",
            Self::Replay(_) => "replay",
        }
    }

    /// Whether this invocation asked for machine-readable output.
    pub fn json(&self) -> bool {
        match self {
            Self::New(args) => args.json,
            Self::Run(args) => args.json,
            Self::Build(args) => args.json,
            Self::Screenshot(args) => args.json,
            Self::Replay(args) => args.json,
        }
    }
}

/// `crcbl new`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewArgs {
    /// Project and package name.
    pub name: String,
    /// Parent directory. Defaults to the current directory.
    pub path: Option<PathBuf>,
    /// Engine checkout to depend on. Defaults to a search upwards from the
    /// current directory.
    pub engine: Option<PathBuf>,
    /// Overwrite a non-empty destination.
    pub force: bool,
    /// Machine-readable output.
    pub json: bool,
}

/// `crcbl run`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunArgs {
    /// Pass `--headless` to the game.
    pub headless: bool,
    /// Cargo package to run.
    pub package: Option<String>,
    /// Build with optimizations.
    pub release: bool,
    /// Machine-readable output.
    pub json: bool,
    /// Everything after `--`.
    pub passthrough: Vec<String>,
}

/// `crcbl build`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildArgs {
    /// What to build for.
    pub target: Target,
    /// Cargo package to build.
    pub package: Option<String>,
    /// Build with optimizations.
    pub release: bool,
    /// Machine-readable output.
    pub json: bool,
}

/// What `crcbl build --target` accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    /// This machine. The default.
    #[default]
    Native,
    /// The wasm/Pages bundle, which lands at P5.
    ///
    /// Recognized so it can be *refused* with a phase rather than an "unknown
    /// target" — the difference between "not yet" and "never" matters to
    /// someone reading a CI log.
    Wasm,
}

/// `crcbl screenshot`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenshotArgs {
    /// Output width.
    pub width: u32,
    /// Output height.
    pub height: u32,
    /// Output PNG path.
    pub output: std::path::PathBuf,
    /// Machine-readable output.
    pub json: bool,
}

/// `crcbl replay`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayArgs {
    /// Path to the .crpl file.
    pub file: PathBuf,
    /// Machine-readable output.
    pub json: bool,
}

/// `crcbl replay --help`.
pub const REPLAY_USAGE: &str = "\
crcbl replay — read a .crpl replay file and dump its metadata

USAGE:
    crcbl replay <FILE> [OPTIONS]

OPTIONS:
        --json    Emit one JSON object instead of human output.
    -h, --help    Print this text.";

/// Parses arguments, which must **not** include the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Invocation {
    let mut args = args.into_iter().peekable();
    let Some(first) = args.next() else {
        return Invocation::BadUsage("no command given".to_string());
    };

    match first.as_str() {
        "-h" | "--help" | "help" => Invocation::Help(USAGE),
        "-V" | "--version" => Invocation::Version,
        "new" => parse_new(args),
        "run" => parse_run(args),
        "build" => parse_build(args),
        "screenshot" => parse_screenshot(args),
        "replay" => parse_replay(args),
        other if other.starts_with('-') => {
            Invocation::BadUsage(format!("unrecognized option `{other}`"))
        }
        other => Invocation::BadUsage(format!("unrecognized command `{other}`")),
    }
}

fn parse_new(mut args: impl Iterator<Item = String>) -> Invocation {
    let mut parsed = NewArgs {
        name: String::new(),
        path: None,
        engine: None,
        force: false,
        json: false,
    };
    let mut name = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Invocation::Help(NEW_USAGE),
            "--json" => parsed.json = true,
            "--force" => parsed.force = true,
            "--path" => match args.next() {
                Some(value) => parsed.path = Some(PathBuf::from(value)),
                None => return bad("--path needs a directory"),
            },
            "--engine" => match args.next() {
                Some(value) => parsed.engine = Some(PathBuf::from(value)),
                None => return bad("--engine needs a directory"),
            },
            other if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`new` has no option `{other}`"));
            }
            other if name.is_none() => name = Some(other.to_string()),
            other => {
                return Invocation::BadUsage(format!(
                    "`new` takes one name; `{other}` is a second one"
                ));
            }
        }
    }

    let Some(name) = name else {
        return bad("`new` needs a project name");
    };
    if let Err(reason) = check_name(&name) {
        return Invocation::BadUsage(format!("`{name}` is not a usable project name: {reason}"));
    }
    parsed.name = name;
    Invocation::Command(Command::New(parsed))
}

fn parse_run(mut args: impl Iterator<Item = String>) -> Invocation {
    let mut parsed = RunArgs {
        headless: false,
        package: None,
        release: false,
        json: false,
        passthrough: Vec::new(),
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Invocation::Help(RUN_USAGE),
            "--json" => parsed.json = true,
            "--headless" => parsed.headless = true,
            "--release" => parsed.release = true,
            "-p" | "--package" => match args.next() {
                Some(value) => parsed.package = Some(value),
                None => return bad("--package needs a name"),
            },
            // Everything after `--` belongs to the game, including things that
            // look like our own flags.
            "--" => {
                parsed.passthrough.extend(args);
                break;
            }
            other => {
                return Invocation::BadUsage(format!(
                    "`run` has no argument `{other}` (pass game arguments after `--`)"
                ));
            }
        }
    }
    Invocation::Command(Command::Run(parsed))
}

fn parse_build(mut args: impl Iterator<Item = String>) -> Invocation {
    let mut parsed = BuildArgs {
        target: Target::Native,
        package: None,
        release: false,
        json: false,
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Invocation::Help(BUILD_USAGE),
            "--json" => parsed.json = true,
            "--release" => parsed.release = true,
            "-p" | "--package" => match args.next() {
                Some(value) => parsed.package = Some(value),
                None => return bad("--package needs a name"),
            },
            "--target" => match args.next().as_deref() {
                Some("native") => parsed.target = Target::Native,
                Some("wasm" | "wasm32" | "web") => parsed.target = Target::Wasm,
                Some(other) => {
                    return Invocation::BadUsage(format!(
                        "unknown target `{other}` (known: native, wasm)"
                    ));
                }
                None => return bad("--target needs a value"),
            },
            other => {
                return Invocation::BadUsage(format!("`build` has no argument `{other}`"));
            }
        }
    }
    Invocation::Command(Command::Build(parsed))
}

fn parse_screenshot(mut args: impl Iterator<Item = String>) -> Invocation {
    let mut parsed = ScreenshotArgs {
        width: 1920,
        height: 1080,
        output: std::path::PathBuf::from("screenshot.png"),
        json: false,
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Invocation::Help(SCREENSHOT_USAGE),
            "--json" => parsed.json = true,
            "--size" => match args.next() {
                Some(value) => match parse_size(&value) {
                    Some((w, h)) => {
                        parsed.width = w;
                        parsed.height = h;
                    }
                    None => {
                        return Invocation::BadUsage(format!(
                            "`--size` expects WxH, e.g. 1920x1080; got `{value}`"
                        ));
                    }
                },
                None => return bad("--size needs a value"),
            },
            "-o" | "--output" => match args.next() {
                Some(value) => parsed.output = std::path::PathBuf::from(value),
                None => return bad("--output needs a path"),
            },
            other => {
                return Invocation::BadUsage(format!("`screenshot` has no argument `{other}`"));
            }
        }
    }
    Invocation::Command(Command::Screenshot(parsed))
}

fn parse_replay(mut args: impl Iterator<Item = String>) -> Invocation {
    let mut parsed = ReplayArgs {
        file: PathBuf::new(),
        json: false,
    };
    let mut file = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Invocation::Help(REPLAY_USAGE),
            "--json" => parsed.json = true,
            other if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`replay` has no option `{other}`"));
            }
            other if file.is_none() => file = Some(PathBuf::from(other)),
            other => {
                return Invocation::BadUsage(format!(
                    "`replay` takes one file; `{other}` is a second one"
                ));
            }
        }
    }

    let Some(file) = file else {
        return bad("`replay` needs a .crpl file path");
    };
    parsed.file = file;
    Invocation::Command(Command::Replay(parsed))
}

fn parse_size(raw: &str) -> Option<(u32, u32)> {
    let (w, rest) = raw.split_once('x')?;
    let w: u32 = w.parse().ok()?;
    let h: u32 = rest.parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

fn bad(message: &str) -> Invocation {
    Invocation::BadUsage(message.to_string())
}

/// Whether `name` can be both a directory name and a Cargo package name.
///
/// Checked here, at parse time, because a name that only fails once half the
/// files are written is the worst possible time to find out.
fn check_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("it is empty");
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
    {
        return Err("only letters, digits, `-` and `_` are allowed");
    }
    if name.starts_with(|character: char| character.is_ascii_digit()) {
        return Err("a crate name cannot start with a digit");
    }
    if name.starts_with('-') {
        return Err("it would be read as an option");
    }
    // The keywords that actually collide with a crate name. `cargo new` checks
    // the full list; this is the subset someone plausibly types.
    const RESERVED: &[&str] = &[
        "crate",
        "self",
        "super",
        "extern",
        "test",
        "main",
        "core",
        "std",
        "alloc",
        "proc_macro",
    ];
    if RESERVED.contains(&name) {
        return Err("it is a Rust keyword or a reserved crate name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Invocation {
        parse(args.iter().map(|arg| (*arg).to_string()))
    }

    fn command(args: &[&str]) -> Command {
        match parse_args(args) {
            Invocation::Command(command) => command,
            other => panic!("expected a command, got {other:?}"),
        }
    }

    #[test]
    fn the_three_p0_subcommands_parse() {
        assert!(matches!(command(&["new", "game"]), Command::New(_)));
        assert!(matches!(command(&["run"]), Command::Run(_)));
        assert!(matches!(command(&["build"]), Command::Build(_)));
    }

    /// `--json` on *every* subcommand is a stated design rule, so it is tested
    /// as one rather than per command.
    #[test]
    fn json_is_accepted_by_every_subcommand() {
        for args in [
            vec!["new", "game", "--json"],
            vec!["run", "--json"],
            vec!["build", "--json"],
            vec!["screenshot", "--json"],
            vec!["replay", "file.crpl", "--json"],
        ] {
            assert!(command(&args).json(), "{args:?} should have set --json");
        }
    }

    #[test]
    fn new_takes_a_name_and_its_options() {
        let Command::New(args) = command(&[
            "new",
            "mygame",
            "--path",
            "/tmp/x",
            "--engine",
            "/src/crcbl",
            "--force",
        ]) else {
            panic!("expected new");
        };
        assert_eq!(args.name, "mygame");
        assert_eq!(args.path.as_deref(), Some(std::path::Path::new("/tmp/x")));
        assert_eq!(
            args.engine.as_deref(),
            Some(std::path::Path::new("/src/crcbl"))
        );
        assert!(args.force);
    }

    #[test]
    fn run_passes_everything_after_a_double_dash_to_the_game() {
        let Command::Run(args) = command(&[
            "run",
            "--headless",
            "--",
            "--headless",
            "--frames",
            "3",
            "--json",
        ]) else {
            panic!("expected run");
        };
        assert!(args.headless);
        assert!(!args.json, "`--json` after `--` belongs to the game");
        assert_eq!(args.passthrough, ["--headless", "--frames", "3", "--json"]);
    }

    /// wasm is P5, and the difference between "unknown" and "not yet" is the
    /// difference between a typo and a roadmap.
    #[test]
    fn wasm_is_a_recognized_target_and_anything_else_is_not() {
        let Command::Build(args) = command(&["build", "--target", "wasm"]) else {
            panic!("expected build");
        };
        assert_eq!(args.target, Target::Wasm);
        assert!(matches!(
            parse_args(&["build", "--target", "ps5"]),
            Invocation::BadUsage(_)
        ));
    }

    #[test]
    fn help_and_version_are_reachable_from_everywhere() {
        assert_eq!(parse_args(&["--help"]), Invocation::Help(USAGE));
        assert_eq!(parse_args(&["help"]), Invocation::Help(USAGE));
        assert_eq!(parse_args(&["new", "--help"]), Invocation::Help(NEW_USAGE));
        assert_eq!(parse_args(&["run", "-h"]), Invocation::Help(RUN_USAGE));
        assert_eq!(
            parse_args(&["build", "--help"]),
            Invocation::Help(BUILD_USAGE)
        );
        assert_eq!(parse_args(&["-V"]), Invocation::Version);
    }

    /// Exit code 2 is a contract; every path to it is enumerated so none of
    /// them can silently become "succeeded with a default".
    #[test]
    fn bad_invocations_are_named() {
        for args in [
            vec![],
            vec!["frobnicate"],
            vec!["--nope"],
            vec!["new"],
            vec!["new", "a", "b"],
            vec!["new", "game", "--path"],
            vec!["new", "game", "--engine"],
            vec!["new", "game", "--wat"],
            vec!["run", "stray"],
            vec!["run", "-p"],
            vec!["build", "--target"],
            vec!["build", "--target", "ps5"],
            vec!["build", "stray"],
            vec!["replay"],
        ] {
            assert!(
                matches!(parse_args(&args), Invocation::BadUsage(_)),
                "{args:?} should be a bad invocation"
            );
        }
    }

    #[test]
    fn project_names_are_checked_before_anything_is_written() {
        for name in ["", "1game", "my game", "my.game", "crate", "std", "-x"] {
            assert!(
                matches!(parse_args(&["new", name]), Invocation::BadUsage(_)),
                "`{name}` should be rejected"
            );
        }
        for name in ["mygame", "my-game", "my_game", "game2"] {
            assert!(
                matches!(parse_args(&["new", name]), Invocation::Command(_)),
                "`{name}` should be accepted"
            );
        }
    }
}
