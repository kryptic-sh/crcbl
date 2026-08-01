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

//! # Arguments are `OsString`s, not `String`s
//!
//! `std::env::args()` panics on an argument that is not valid Unicode, which on
//! Linux is any path a filesystem will happily hand you. Every flag this CLI
//! takes a *path* for — `--path`, `--engine`, `-o`, and `replay`'s file — would
//! therefore abort with exit 101 rather than the contracted exit 2 or a working
//! command. So the parser reads `OsString`s and only ever asks for UTF-8 where
//! the value genuinely has to be text: a subcommand name, a flag, a Cargo
//! package name, a `WxH` size. Those cases fail as a bad invocation, with the
//! offending argument rendered lossily so the message is still readable.

use std::ffi::OsString;
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
        --size WxH       Output dimensions. Default: 1920x1080. Each edge must
                         be between 1 and 16384.
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
    ///
    /// `OsString` because these are the *game's* arguments and the game's
    /// arguments include paths.
    pub passthrough: Vec<OsString>,
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
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Invocation {
    let mut args = args.into_iter().peekable();
    let Some(first) = args.next() else {
        return Invocation::BadUsage("no command given".to_string());
    };

    match first.to_str() {
        Some("-h" | "--help" | "help") => Invocation::Help(USAGE),
        Some("-V" | "--version") => Invocation::Version,
        Some("new") => parse_new(args),
        Some("run") => parse_run(args),
        Some("build") => parse_build(args),
        Some("screenshot") => parse_screenshot(args),
        Some("replay") => parse_replay(args),
        Some(other) if other.starts_with('-') => {
            Invocation::BadUsage(format!("unrecognized option `{other}`"))
        }
        // A command name is never a path, so a non-UTF-8 one is a typo rather
        // than something to support. It is still exit 2, not a panic.
        Some(other) => Invocation::BadUsage(format!("unrecognized command `{other}`")),
        None => Invocation::BadUsage(format!(
            "unrecognized command `{}`",
            first.to_string_lossy()
        )),
    }
}

/// A non-UTF-8 argument where only text will do.
fn not_text(command: &str, what: &str, arg: &OsString) -> Invocation {
    Invocation::BadUsage(format!(
        "`{command}` needs a {what} that is valid UTF-8; `{}` is not",
        arg.to_string_lossy()
    ))
}

fn parse_new(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = NewArgs {
        name: String::new(),
        path: None,
        engine: None,
        force: false,
        json: false,
    };
    let mut name = None;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(NEW_USAGE),
            Some("--json") => parsed.json = true,
            Some("--force") => parsed.force = true,
            // A directory, so it stays an `OsString` all the way to `PathBuf`.
            Some("--path") => match args.next() {
                Some(value) => parsed.path = Some(PathBuf::from(value)),
                None => return bad("--path needs a directory"),
            },
            Some("--engine") => match args.next() {
                Some(value) => parsed.engine = Some(PathBuf::from(value)),
                None => return bad("--engine needs a directory"),
            },
            Some(other) if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`new` has no option `{other}`"));
            }
            Some(other) if name.is_none() => name = Some(other.to_string()),
            Some(other) => {
                return Invocation::BadUsage(format!(
                    "`new` takes one name; `{other}` is a second one"
                ));
            }
            // A project name becomes a crate name, which `check_name` already
            // restricts to ASCII; a non-UTF-8 one could never have passed.
            None => return not_text("new", "project name", &arg),
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

fn parse_run(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = RunArgs {
        headless: false,
        package: None,
        release: false,
        json: false,
        passthrough: Vec::new(),
    };

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(RUN_USAGE),
            Some("--json") => parsed.json = true,
            Some("--headless") => parsed.headless = true,
            Some("--release") => parsed.release = true,
            // A Cargo package name, which Cargo itself requires to be UTF-8.
            Some("-p" | "--package") => match args.next() {
                Some(value) => match value.into_string() {
                    Ok(name) => parsed.package = Some(name),
                    Err(value) => return not_text("run", "package name", &value),
                },
                None => return bad("--package needs a name"),
            },
            // Everything after `--` belongs to the game, including things that
            // look like our own flags — and including paths, so it is not
            // required to be UTF-8.
            Some("--") => {
                parsed.passthrough.extend(args);
                break;
            }
            Some(other) => {
                return Invocation::BadUsage(format!(
                    "`run` has no argument `{other}` (pass game arguments after `--`)"
                ));
            }
            None => {
                return Invocation::BadUsage(format!(
                    "`run` has no argument `{}` (pass game arguments after `--`)",
                    arg.to_string_lossy()
                ));
            }
        }
    }
    Invocation::Command(Command::Run(parsed))
}

fn parse_build(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = BuildArgs {
        target: Target::Native,
        package: None,
        release: false,
        json: false,
    };

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(BUILD_USAGE),
            Some("--json") => parsed.json = true,
            Some("--release") => parsed.release = true,
            Some("-p" | "--package") => match args.next() {
                Some(value) => match value.into_string() {
                    Ok(name) => parsed.package = Some(name),
                    Err(value) => return not_text("build", "package name", &value),
                },
                None => return bad("--package needs a name"),
            },
            Some("--target") => {
                let Some(value) = args.next() else {
                    return bad("--target needs a value");
                };
                match value.to_str() {
                    Some("native") => parsed.target = Target::Native,
                    Some("wasm" | "wasm32" | "web") => parsed.target = Target::Wasm,
                    Some(other) => {
                        return Invocation::BadUsage(format!(
                            "unknown target `{other}` (known: native, wasm)"
                        ));
                    }
                    None => {
                        return Invocation::BadUsage(format!(
                            "unknown target `{}` (known: native, wasm)",
                            value.to_string_lossy()
                        ));
                    }
                }
            }
            Some(other) => {
                return Invocation::BadUsage(format!("`build` has no argument `{other}`"));
            }
            None => {
                return Invocation::BadUsage(format!(
                    "`build` has no argument `{}`",
                    arg.to_string_lossy()
                ));
            }
        }
    }
    Invocation::Command(Command::Build(parsed))
}

fn parse_screenshot(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = ScreenshotArgs {
        width: 1920,
        height: 1080,
        output: std::path::PathBuf::from("screenshot.png"),
        json: false,
    };

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(SCREENSHOT_USAGE),
            Some("--json") => parsed.json = true,
            Some("--size") => {
                let Some(value) = args.next() else {
                    return bad("--size needs a value");
                };
                let raw = value.to_string_lossy().into_owned();
                match value.to_str().map(parse_size) {
                    Some(Ok((width, height))) => {
                        parsed.width = width;
                        parsed.height = height;
                    }
                    Some(Err(reason)) => return Invocation::BadUsage(reason),
                    None => return Invocation::BadUsage(size_syntax(&raw)),
                }
            }
            // A path, so it stays an `OsString` all the way to `PathBuf`.
            Some("-o" | "--output") => match args.next() {
                Some(value) => parsed.output = std::path::PathBuf::from(value),
                None => return bad("--output needs a path"),
            },
            Some(other) => {
                return Invocation::BadUsage(format!("`screenshot` has no argument `{other}`"));
            }
            None => {
                return Invocation::BadUsage(format!(
                    "`screenshot` has no argument `{}`",
                    arg.to_string_lossy()
                ));
            }
        }
    }
    Invocation::Command(Command::Screenshot(parsed))
}

fn parse_replay(args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = ReplayArgs {
        file: PathBuf::new(),
        json: false,
    };
    let mut file = None;

    for arg in args {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(REPLAY_USAGE),
            Some("--json") => parsed.json = true,
            Some(other) if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`replay` has no option `{other}`"));
            }
            Some(_) | None if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Invocation::BadUsage(format!(
                    "`replay` takes one file; `{}` is a second one",
                    arg.to_string_lossy()
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

/// `WxH`, bounded on both ends.
///
/// The upper bound is [`crcbl::screenshot::MAX_DIMENSION`] and it is enforced
/// *here*, at parse time, so `--size 4000000000x4000000000` and `--size
/// 100000x100000` are bad invocations (exit 2) rather than a 40 GB allocation
/// or an overflowed byte count several layers down. The engine checks the same
/// bound again for callers that never went through this parser.
fn parse_size(raw: &str) -> Result<(u32, u32), String> {
    const MAX: u32 = crcbl::screenshot::MAX_DIMENSION;

    let Some((width, height)) = raw.split_once('x') else {
        return Err(size_syntax(raw));
    };
    let (Ok(width), Ok(height)) = (width.parse::<u32>(), height.parse::<u32>()) else {
        return Err(size_syntax(raw));
    };
    if width == 0 || height == 0 {
        return Err(format!(
            "`--size` needs a non-zero width and height; got `{raw}`"
        ));
    }
    if width > MAX || height > MAX {
        return Err(format!(
            "`--size` is capped at {MAX}x{MAX} (a {MAX}x{MAX} frame is already a gibibyte of \
             pixels); got `{raw}`"
        ));
    }
    Ok((width, height))
}

fn size_syntax(raw: &str) -> String {
    format!("`--size` expects WxH, e.g. 1920x1080; got `{raw}`")
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
        parse(args.iter().map(OsString::from))
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
        assert_eq!(
            args.passthrough,
            ["--headless", "--frames", "3", "--json"].map(OsString::from)
        );
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
            vec!["screenshot", "--size"],
            vec!["screenshot", "--size", "1920"],
            vec!["screenshot", "--size", "nonsensexthing"],
            vec!["screenshot", "--size", "0x1080"],
            vec!["screenshot", "--size", "1920x0"],
            // The two the review names: an overflowing product and a 40 GB
            // allocation. Both are exit 2, not a panic and not an OOM.
            vec!["screenshot", "--size", "4000000000x4000000000"],
            vec!["screenshot", "--size", "100000x100000"],
            vec!["screenshot", "--size", "16385x16"],
            vec!["screenshot", "--size", "16x16385"],
            vec!["screenshot", "-o"],
        ] {
            assert!(
                matches!(parse_args(&args), Invocation::BadUsage(_)),
                "{args:?} should be a bad invocation"
            );
        }
    }

    /// The help text quotes the cap as a literal, so it is pinned to the
    /// constant it describes.
    #[test]
    fn the_screenshot_help_names_the_real_size_cap() {
        let max = crcbl::screenshot::MAX_DIMENSION;
        assert!(
            SCREENSHOT_USAGE.contains(&max.to_string()),
            "`screenshot --help` must name the {max} cap:\n{SCREENSHOT_USAGE}"
        );
    }

    /// The largest frame the engine will render is still a *good* invocation;
    /// the bound is a cap, not an off-by-one.
    #[test]
    fn the_largest_accepted_size_is_the_engines_own_limit() {
        let max = crcbl::screenshot::MAX_DIMENSION;
        let Command::Screenshot(args) = command(&["screenshot", "--size", &format!("{max}x{max}")])
        else {
            panic!("expected screenshot");
        };
        assert_eq!((args.width, args.height), (max, max));
    }

    /// `std::env::args()` panics on a non-UTF-8 argument, and every one of
    /// these flags takes a path. A path a filesystem accepts must reach the
    /// command intact, and a non-UTF-8 value where only text will do must be
    /// exit 2 rather than exit 101.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_path_survives_and_a_non_utf8_name_is_exit_two() {
        use std::os::unix::ffi::OsStringExt;

        let weird = || OsString::from_vec(b"/tmp/n\xfft-utf8".to_vec());
        let parse_os = |args: Vec<OsString>| parse(args);

        let Invocation::Command(Command::New(args)) = parse_os(vec![
            OsString::from("new"),
            OsString::from("mygame"),
            OsString::from("--path"),
            weird(),
            OsString::from("--engine"),
            weird(),
        ]) else {
            panic!("a non-UTF-8 --path/--engine is a usable invocation");
        };
        assert_eq!(args.path.as_deref(), Some(std::path::Path::new(&weird())));
        assert_eq!(args.engine.as_deref(), Some(std::path::Path::new(&weird())));

        let Invocation::Command(Command::Screenshot(args)) = parse_os(vec![
            OsString::from("screenshot"),
            OsString::from("-o"),
            weird(),
        ]) else {
            panic!("a non-UTF-8 -o is a usable invocation");
        };
        assert_eq!(args.output, std::path::PathBuf::from(weird()));

        let Invocation::Command(Command::Replay(args)) =
            parse_os(vec![OsString::from("replay"), weird()])
        else {
            panic!("a non-UTF-8 replay file is a usable invocation");
        };
        assert_eq!(args.file, std::path::PathBuf::from(weird()));

        // Game arguments are the game's business, paths included.
        let Invocation::Command(Command::Run(args)) = parse_os(vec![
            OsString::from("run"),
            OsString::from("--"),
            OsString::from("--scene"),
            weird(),
        ]) else {
            panic!("expected run");
        };
        assert_eq!(args.passthrough, vec![OsString::from("--scene"), weird()]);

        // And where the value has to be text, it is a named bad invocation.
        for args in [
            vec![weird()],
            vec![OsString::from("new"), weird()],
            vec![OsString::from("build"), OsString::from("-p"), weird()],
            vec![OsString::from("build"), OsString::from("--target"), weird()],
            vec![
                OsString::from("screenshot"),
                OsString::from("--size"),
                weird(),
            ],
        ] {
            assert!(
                matches!(parse_os(args.clone()), Invocation::BadUsage(_)),
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
