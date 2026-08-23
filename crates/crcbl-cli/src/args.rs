//! Argument parsing, and the decision not to take `clap`.
//!
//! # Decision: hand-rolled, revisited when the CLI reaches its P9 shape
//!
//! `clap` is the obvious choice, it is well maintained, its licence passes
//! `deny.toml`, and it is not a framework that would own engine policy. It was
//! still rejected for P0, on four grounds:
//!
//! 1. **Size, for what it buys.** `clap` with its default features brings a
//!    dependency tree of its own to parse this CLI's subcommands and flags,
//!    which are one `match`. **This ground is weaker than it was written**, and
//!    saying so is the point of recording it: the original text called `clap`
//!    "by a wide margin, the largest dependency in the engine" against a
//!    workspace whose third-party list it then enumerated. That list is far
//!    longer now — `ash`, `gltf` and `serde` are each larger than `clap` — so
//!    the superlative is false and only the ratio survives. Read
//!    `[workspace.dependencies]` in the root `Cargo.toml` for what the baseline
//!    actually is rather than trusting a sentence here.
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
//!    with interlocking flags. At the handful this CLI has, they are unused
//!    surface.
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

use crcbl_sprite::{NineSlice, SampleMode};

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
    crpix         Convert PNG frames into one .crpix sprite sheet.
    lod           Report or generate a glTF mesh's LOD chain.
    bench         Run a fixed benchmark scenario and report its distribution.

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
        --scene <SCENE>  What to draw. Each name is the file stem of the golden
                         the render e2e blesses for it, so a frame taken here
                         and the one CI compares are the same scene under the
                         same name:
                           cube (the default)  lit cube, three pyramids
                           dunes               the cluster-DAG height field
                           lights              the cube scene under three
                                               coloured point lights
                           spot                one spot cone on a floor
                           spot_shadow         that cone with a caster in it
                           point_shadow        one point light, two casters
                           ao                  the inside of a box, ambient only
                           ssr                 a pyramid reflected in a smooth
                                               floor
                           probes              a room lit by irradiance probes
                           sprite              four sprites over three batches
                           ui                  text, a rect and an outline
        --size WxH       Output dimensions. Default: 1920x1080. Each edge must
                         be between 1 and 16384.
    -o, --output <FILE>  Write the PNG here. Default: screenshot.png.
        --json           Emit one JSON object instead of human output.
    -h, --help           Print this text.";

/// `crcbl crpix --help`.
pub const CRPIX_USAGE: &str = "\
crcbl crpix — convert PNG frames into one .crpix sprite sheet

USAGE:
    crcbl crpix <PNG>... -o <FILE> [OPTIONS]

The images become the sheet's frames, in the order given on the command line,
and every one of them must be the same size — a mismatch names the file rather
than padding it. Each frame is named after its file stem, so `art/bird-up.png`
becomes the frame `bird-up`, and two inputs whose stems collide are an error:
a clip could not tell the two apart. A stem that is empty, is not text, or
carries whitespace, `:` or `#` is refused for the same reason — the format
could not spell it back.

    crcbl crpix up.png mid.png down.png -o bird.crpix --clip flap --hold 6

OPTIONS:
    -o, --output <FILE>   Write the .crpix here. Required; an existing file is
                          left alone unless --force is given.
        --force           Overwrite the output file if it already exists.
        --nine <L,R,T,B>  Nine-slice insets in pixels, left, right, top,
                          bottom. Default: no nine-slice.
        --sample <MODE>   `pixel` (the default) or `smooth`.
        --clip <NAME>     Also write a looping clip named NAME over every
                          frame, in order. Default: no clip.
        --hold <TICKS>    Ticks that clip holds each frame for. Default: 1.
                          Needs --clip, which is the only thing that reads it.
        --json            Emit one JSON object instead of human output.
    -h, --help            Print this text.";

/// `crcbl lod --help`.
pub const LOD_USAGE: &str = "\
crcbl lod — report or generate a glTF mesh's LOD chain

USAGE:
    crcbl lod stats <FILE> [OPTIONS]
    crcbl lod gen <FILE> -o <FILE> [OPTIONS]

`stats` resolves the LOD chain of every mesh the file draws and reports, per
level, where the geometry came from — the file's own, or the cluster-DAG
generator — alongside its triangle and cluster counts and its error range, then
the shape of each DAG behind it: groups per level, and whether the levels really
halve or stall.

`gen` builds one mesh primitive's cluster DAG headlessly and writes it as a
cooked .dag artifact, the format `crates/crcbl-shaders/clusters/*.dag` is in.

`preview` — offscreen renders per level, from `docs/plan/25-lod.md` — is not
implemented. It is recognized so that asking for it fails saying so rather than
looking like a typo.

FILE is a .gltf or .glb, and its file name has to be a legal asset key: ASCII
letters, digits, `.`, `_` and `-`. That is the rule every asset this engine
loads obeys, so that a tree which loads from a directory also loads over HTTP.

OPTIONS:
        --node <INDEX>       Work on this node alone. Default: `stats` reports
                             every node that draws a mesh, `gen` takes the only
                             one and refuses a file with several.
        --primitive <INDEX>  (gen) Which primitive of that node's mesh to build
                             a DAG for. Default: the only one, or a refusal.
    -o, --output <FILE>      (gen) Write the .dag here. Required; an existing
                             file is left alone unless --force is given.
        --force              (gen) Overwrite the output file if it exists.
        --json               Emit one JSON object instead of human output.
    -h, --help               Print this text.";

/// `crcbl bench --help`.
pub const BENCH_USAGE: &str = "\
crcbl bench — run a fixed benchmark scenario and report its distribution

USAGE:
    crcbl bench --scenario <NAME> [OPTIONS]

One scenario per invocation, run headless, with the warm-up excluded from the
statistics and the environment printed beside them — a number without the
machine it came from is not comparable to another number.

SCENARIOS:
    jobs    `crcbl_jobs::Pool::par_for` over a fixed synthetic workload, timed
            per call. `--workers 0` is the serial baseline the parallel numbers
            only mean something against.

OPTIONS:
        --scenario <NAME>    Which scenario to run. Required.
        --workers <N>        Pool worker threads. Default: one fewer than the
                             machine's parallelism, which is what `Pool::new`
                             asks for. 0 runs every chunk on the calling thread.
        --items <N>          Items in the workload. Default: 10000.
        --chunk <N>          Items per `par_for` chunk, and the thing worth
                             sweeping. Default: 64.
        --iterations <N>     Timed calls. Default: 200. Below 20 the run reports
                             its maximum and no percentile.
        --warmup <N>         Untimed calls first, excluded from the statistics
                             and from the pool counters. Default: 20.
        --json               Emit one JSON object instead of human output.
    -h, --help               Print this text.";

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

/// One of the CLI's subcommands.
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
    /// PNG frames → one .crpix sheet.
    Crpix(CrpixArgs),
    /// A glTF mesh's LOD chain, reported or generated.
    Lod(LodArgs),
    /// One fixed benchmark scenario, timed.
    Bench(BenchArgs),
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
            Self::Crpix(_) => "crpix",
            // One name for the whole subcommand tree, with the branch reported
            // as a field of its own — see [`LodAction::name`]. A `"command"`
            // that varied with an inner verb would make every consumer match on
            // a set of strings that grows with the CLI.
            Self::Lod(_) => "lod",
            // The scenario is a field of its own, for the reason above.
            Self::Bench(_) => "bench",
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
            Self::Crpix(args) => args.json,
            Self::Lod(args) => args.json,
            Self::Bench(args) => args.json,
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
    /// The wasm/Pages bundle, which `web/build.sh` builds and this does not.
    ///
    /// Recognized so it can be *refused* by name, pointing at the script that
    /// assembles the bundle, rather than as an "unknown target" — the
    /// difference between "wrong tool" and "typo" matters to someone reading a
    /// CI log. See [`crate::cargo`]'s module docs for why it is not just a
    /// `cargo build` with a `--target` flag.
    Wasm,
}

/// `crcbl screenshot`.
///
/// The scene is `crcbl::screenshot`'s own enum rather than a parallel one, for
/// the reason [`CrpixArgs`] gives about the nine-slice: a second enum meaning
/// the same thing is a translation layer that can only ever drift.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenshotArgs {
    /// Output width.
    pub width: u32,
    /// Output height.
    pub height: u32,
    /// What to draw.
    pub scene: crcbl::screenshot::Scene,
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

/// `crcbl crpix`.
///
/// The nine-slice and the sample mode are `crcbl_sprite`'s own types rather
/// than a pair of parallel ones: they are exactly what `trace::Options` takes,
/// and a second enum meaning the same thing is a translation layer that can
/// only ever drift.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrpixArgs {
    /// The PNGs, in frame order. Never empty — the parser requires one.
    pub inputs: Vec<PathBuf>,
    /// Where the `.crpix` goes. Required; there is no default.
    pub output: PathBuf,
    /// Overwrite an existing output file.
    pub force: bool,
    /// Nine-slice insets to write into the sheet.
    pub nine: Option<NineSlice>,
    /// How the sheet asks to be sampled.
    pub sample: SampleMode,
    /// Name of a clip over every frame, in order. `None` writes no clip.
    pub clip: Option<String>,
    /// Ticks that clip holds each frame for. Meaningless without `clip`, which
    /// is why supplying it alone is a bad invocation rather than a no-op.
    pub hold: u32,
    /// Machine-readable output.
    pub json: bool,
}

/// Which half of `crcbl lod` was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LodAction {
    /// Report the chain and the DAG behind it.
    Stats,
    /// Build one primitive's DAG and write the cooked artifact.
    Gen,
    /// Offscreen renders per level.
    ///
    /// Recognized so it can be *refused* with a reason rather than as an
    /// unknown subcommand — the difference between "not yet" and "never", the
    /// same distinction [`Target::Wasm`] is parsed to make about "wrong tool".
    Preview,
}

impl LodAction {
    /// The name this branch is reported under in `--json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Stats => "stats",
            Self::Gen => "gen",
            Self::Preview => "preview",
        }
    }
}

/// `crcbl lod`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LodArgs {
    /// Which branch to run.
    pub action: LodAction,
    /// The glTF to read.
    pub file: PathBuf,
    /// Restrict to one node of the scene. `gen` needs exactly one and takes it
    /// from here when the file has several.
    pub node: Option<usize>,
    /// Which primitive of that node's mesh `gen` builds. `None` is "the only
    /// one", which is a refusal when there are several.
    pub primitive: Option<usize>,
    /// Where `gen` writes the `.dag`. Always `Some` for
    /// [`LodAction::Gen`] and always `None` otherwise — the parser refuses the
    /// combinations that are neither.
    pub output: Option<PathBuf>,
    /// Overwrite an existing output file.
    pub force: bool,
    /// Machine-readable output.
    pub json: bool,
}

/// Which fixed workload `crcbl bench` runs.
///
/// `docs/plan/40-profiling.md` requires scenarios to be "named and fixed", so
/// this is an enum and not a free-form string: a name that answers to nothing is
/// refused at parse time, with the names that do listed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BenchScenario {
    /// `crcbl_jobs::Pool::par_for` over a synthetic integer workload.
    ///
    /// The first one because the job system is the first thing that needs
    /// proving — the plan's own note against this delivery row.
    #[default]
    Jobs,
}

impl BenchScenario {
    /// The `--scenario` name, and the name this run is reported under.
    ///
    /// Exhaustive on purpose — no wildcard arm — so adding a variant is a
    /// compile error here rather than a scenario the CLI silently cannot run.
    /// The same arrangement as [`scene_name`], and for the same reason.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Jobs => "jobs",
        }
    }
}

/// Every scenario, for the name lookup and the rejection message.
const SCENARIOS: &[BenchScenario] = &[BenchScenario::Jobs];

/// The scenario `name` selects, or `None` if no scenario answers to it.
fn scenario_from_name(name: &str) -> Option<BenchScenario> {
    SCENARIOS
        .iter()
        .copied()
        .find(|&scenario| scenario.name() == name)
}

/// Every name `--scenario` takes, for the rejection message.
///
/// Built from [`SCENARIOS`], so the list a user is shown after a typo cannot
/// name a scenario the parser does not accept.
fn scenario_names() -> String {
    SCENARIOS
        .iter()
        .map(|&scenario| scenario.name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Items the workload holds when `--items` is not given.
///
/// The crowd size `docs/plan/sample/03-horde.md` sets as its exit criterion,
/// because the pass this benchmark stands in for is horde's steering.
const DEFAULT_BENCH_ITEMS: usize = 10_000;

/// Items per chunk when `--chunk` is not given.
///
/// horde's `STEER_CHUNK`, which `docs/backlog.md` records as chosen by argument
/// rather than measurement — so it is the default a sweep starts from, not a
/// value this file is claiming is right.
const DEFAULT_BENCH_CHUNK: usize = 64;

/// Timed calls when `--iterations` is not given.
///
/// Comfortably above [`crcbl::core::stats::MIN_PERCENTILE_SAMPLES`], so the
/// default invocation reports percentiles rather than explaining why it cannot.
const DEFAULT_BENCH_ITERATIONS: usize = 200;

/// Untimed calls when `--warmup` is not given.
const DEFAULT_BENCH_WARMUP: usize = 20;

/// `crcbl bench`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchArgs {
    /// Which workload to run.
    pub scenario: BenchScenario,
    /// Pool workers, or `None` for the count `Pool::new` would pick.
    ///
    /// `Some(0)` is legal and is the serial baseline: every chunk runs on the
    /// calling thread.
    pub workers: Option<usize>,
    /// Items in the workload. Never zero; the parser refuses it.
    pub items: usize,
    /// Items per `par_for` chunk. Never zero, for the same reason.
    pub chunk: usize,
    /// Timed calls. Never zero, for the same reason.
    pub iterations: usize,
    /// Untimed calls first, excluded from everything reported.
    pub warmup: usize,
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
        Some("crpix") => parse_crpix(args),
        Some("lod") => parse_lod(args),
        Some("bench") => parse_bench(args),
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

/// What `--scene` accepts, and what each name draws.
///
/// The name is the **golden's file stem** rather than a spelling of its own:
/// `crates/crcbl/tests/render_e2e.rs` blesses one PNG per scene under exactly
/// these stems, so a frame taken by hand and the one CI compares are reachable
/// by the same word. A second vocabulary would be a translation step, and the
/// place a typo turns into "the cube twice, and every shader agrees".
///
/// **A [`Scene`](crcbl::screenshot::Scene) variant absent from this table is
/// unreachable from the command line.** Nothing here can notice that on its own;
/// what points at it is [`scene_name`], whose match is exhaustive, so a new
/// variant stops this crate compiling until somebody comes to this file.
const SCENES: &[crcbl::screenshot::Scene] = {
    use crcbl::screenshot::Scene;
    &[
        Scene::Cube,
        Scene::Dunes,
        Scene::Lights,
        Scene::Spot,
        Scene::SpotShadow,
        Scene::PointShadow,
        Scene::Ao,
        Scene::Ssr,
        Scene::Probes,
        Scene::Sprite,
        Scene::Ui,
    ]
};

/// The `--scene` name for a scene.
///
/// Exhaustive on purpose — no wildcard arm — so adding a
/// [`Scene`](crcbl::screenshot::Scene) variant is a compile error here rather
/// than a scene the CLI silently cannot draw. See [`SCENES`].
const fn scene_name(scene: crcbl::screenshot::Scene) -> &'static str {
    use crcbl::screenshot::Scene;
    match scene {
        Scene::Cube => "cube",
        Scene::Dunes => "dunes",
        Scene::Lights => "lights",
        Scene::Spot => "spot",
        Scene::SpotShadow => "spot_shadow",
        Scene::PointShadow => "point_shadow",
        Scene::Ao => "ao",
        Scene::Ssr => "ssr",
        Scene::Probes => "probes",
        Scene::Sprite => "sprite",
        Scene::Ui => "ui",
    }
}

/// The scene `name` selects, or `None` if no scene answers to it.
fn scene_from_name(name: &str) -> Option<crcbl::screenshot::Scene> {
    SCENES
        .iter()
        .copied()
        .find(|&scene| scene_name(scene) == name)
}

/// Every name `--scene` takes, for the rejection message.
///
/// Built from [`SCENES`] rather than written out, so the list a user is shown
/// after a typo cannot name a scene the parser does not accept.
fn scene_names() -> String {
    SCENES
        .iter()
        .map(|&scene| scene_name(scene))
        .collect::<Vec<_>>()
        .join(", ")
}

fn parse_screenshot(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = ScreenshotArgs {
        width: 1920,
        height: 1080,
        scene: crcbl::screenshot::Scene::default(),
        output: std::path::PathBuf::from("screenshot.png"),
        json: false,
    };

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(SCREENSHOT_USAGE),
            Some("--json") => parsed.json = true,
            Some("--scene") => {
                let Some(value) = args.next() else {
                    return bad("--scene needs a value");
                };
                match value.to_str().and_then(scene_from_name) {
                    Some(scene) => parsed.scene = scene,
                    None => {
                        return Invocation::BadUsage(format!(
                            "unknown scene `{}` (known: {})",
                            value.to_string_lossy(),
                            scene_names()
                        ));
                    }
                }
            }
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

fn parse_crpix(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = CrpixArgs {
        inputs: Vec::new(),
        output: PathBuf::new(),
        force: false,
        nine: None,
        sample: SampleMode::default(),
        clip: None,
        hold: 1,
        json: false,
    };
    let mut output = None;
    // Tracked rather than inferred from `hold != 1`, so `--hold 1 ` without a
    // clip is refused too: it is the same mistake, and reading as accepted
    // teaches that the pair is optional.
    let mut hold_given = false;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(CRPIX_USAGE),
            Some("--json") => parsed.json = true,
            Some("--force") => parsed.force = true,
            // A path, so it stays an `OsString` all the way to `PathBuf`.
            Some("-o" | "--output") => match args.next() {
                Some(value) => output = Some(PathBuf::from(value)),
                None => return bad("--output needs a path"),
            },
            Some("--nine") => {
                let Some(value) = args.next() else {
                    return bad("--nine needs a value");
                };
                match value.to_str().map(parse_insets) {
                    Some(Ok(nine)) => parsed.nine = Some(nine),
                    Some(Err(reason)) => return Invocation::BadUsage(reason),
                    None => {
                        return Invocation::BadUsage(insets_syntax(&value.to_string_lossy()));
                    }
                }
            }
            Some("--sample") => {
                let Some(value) = args.next() else {
                    return bad("--sample needs a value");
                };
                match value.to_str() {
                    Some("pixel") => parsed.sample = SampleMode::Pixel,
                    Some("smooth") => parsed.sample = SampleMode::Smooth,
                    _ => {
                        return Invocation::BadUsage(format!(
                            "unknown sample mode `{}` (known: pixel, smooth)",
                            value.to_string_lossy()
                        ));
                    }
                }
            }
            // A clip name is written into the file as text, so it has to be
            // text, and it has to be text the format can spell back.
            Some("--clip") => match args.next() {
                Some(value) => match value.into_string() {
                    Ok(name) => match check_sheet_name(&name) {
                        Ok(()) => parsed.clip = Some(name),
                        Err(reason) => {
                            return Invocation::BadUsage(format!(
                                "`{name}` is not a usable clip name: {reason}"
                            ));
                        }
                    },
                    Err(value) => return not_text("crpix", "clip name", &value),
                },
                None => return bad("--clip needs a name"),
            },
            // Zero is refused here rather than passed on: `trace` writes the
            // hold only when it is above 1 and the parser refuses `@ 0`, so a
            // zero would be silently read back as 1 — a flag that did nothing.
            Some("--hold") => {
                let Some(value) = args.next() else {
                    return bad("--hold needs a value");
                };
                match value
                    .to_str()
                    .and_then(|text| text.parse::<u32>().ok())
                    .filter(|ticks| *ticks > 0)
                {
                    Some(ticks) => {
                        parsed.hold = ticks;
                        hold_given = true;
                    }
                    None => {
                        return Invocation::BadUsage(format!(
                            "`--hold` expects a whole number of ticks, at least 1; got `{}`",
                            value.to_string_lossy()
                        ));
                    }
                }
            }
            Some(other) if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`crpix` has no option `{other}`"));
            }
            // An input PNG. A path, so any bytes a filesystem accepts.
            Some(_) | None => parsed.inputs.push(PathBuf::from(arg)),
        }
    }

    let Some(output) = output else {
        return bad("`crpix` needs an output path (-o <FILE>)");
    };
    if parsed.inputs.is_empty() {
        return bad("`crpix` needs at least one PNG");
    }
    if hold_given && parsed.clip.is_none() {
        return bad(
            "`--hold` is the hold of a clip, and `--clip` is what writes one; pass both or \
             neither",
        );
    }
    parsed.output = output;
    Invocation::Command(Command::Crpix(parsed))
}

fn parse_lod(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let Some(first) = args.next() else {
        return bad("`lod` needs a subcommand (stats, gen)");
    };
    let action = match first.to_str() {
        Some("-h" | "--help") => return Invocation::Help(LOD_USAGE),
        Some("stats") => LodAction::Stats,
        Some("gen") => LodAction::Gen,
        Some("preview") => LodAction::Preview,
        _ => {
            return Invocation::BadUsage(format!(
                "`lod` has no subcommand `{}` (known: stats, gen, preview)",
                first.to_string_lossy()
            ));
        }
    };

    let mut parsed = LodArgs {
        action,
        file: PathBuf::new(),
        node: None,
        primitive: None,
        output: None,
        force: false,
        json: false,
    };
    let mut file = None;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(LOD_USAGE),
            Some("--json") => parsed.json = true,
            Some("--force") => parsed.force = true,
            Some("--node") => match index(&mut args, "--node") {
                Ok(value) => parsed.node = Some(value),
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--primitive") => match index(&mut args, "--primitive") {
                Ok(value) => parsed.primitive = Some(value),
                Err(message) => return Invocation::BadUsage(message),
            },
            // A path, so it stays an `OsString` all the way to `PathBuf`.
            Some("-o" | "--output") => match args.next() {
                Some(value) => parsed.output = Some(PathBuf::from(value)),
                None => return bad("--output needs a path"),
            },
            Some(other) if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`lod` has no option `{other}`"));
            }
            // The glTF. A path, so any bytes a filesystem accepts — the *asset
            // key* rules apply to the file name and are enforced by the command,
            // where a refusal can explain itself.
            Some(_) | None if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Invocation::BadUsage(format!(
                    "`lod {}` takes one file; `{}` is a second one",
                    action.name(),
                    arg.to_string_lossy()
                ));
            }
        }
    }

    let Some(file) = file else {
        return Invocation::BadUsage(format!("`lod {}` needs a glTF file", action.name()));
    };
    parsed.file = file;

    // Every flag that belongs to one branch is refused on the others rather
    // than ignored: a flag that silently does nothing teaches that it works.
    if action == LodAction::Gen {
        if parsed.output.is_none() {
            return bad("`lod gen` needs an output path (-o <FILE>)");
        }
    } else {
        if parsed.output.is_some() {
            return Invocation::BadUsage(format!(
                "`lod {}` writes nothing, so it has no --output; `lod gen` is the \
                 half that does",
                action.name()
            ));
        }
        if parsed.force {
            return Invocation::BadUsage(format!(
                "`lod {}` writes nothing, so there is nothing for --force to overwrite",
                action.name()
            ));
        }
        if parsed.primitive.is_some() {
            return Invocation::BadUsage(format!(
                "`lod {}` reports every primitive, so it has no --primitive",
                action.name()
            ));
        }
    }

    Invocation::Command(Command::Lod(parsed))
}

fn parse_bench(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = BenchArgs {
        scenario: BenchScenario::default(),
        workers: None,
        items: DEFAULT_BENCH_ITEMS,
        chunk: DEFAULT_BENCH_CHUNK,
        iterations: DEFAULT_BENCH_ITERATIONS,
        warmup: DEFAULT_BENCH_WARMUP,
        json: false,
    };
    // Separate from `parsed.scenario`'s default so that *omitting* `--scenario`
    // is refused rather than silently running whichever one happens to be first
    // in `SCENARIOS`. A benchmark that ran something other than what was asked
    // for would be the one output nobody could interpret.
    let mut scenario = None;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(BENCH_USAGE),
            Some("--json") => parsed.json = true,
            Some("--scenario") => {
                let Some(value) = args.next() else {
                    return bad("--scenario needs a value");
                };
                match value.to_str().and_then(scenario_from_name) {
                    Some(found) => scenario = Some(found),
                    None => {
                        return Invocation::BadUsage(format!(
                            "unknown scenario `{}` (known: {})",
                            value.to_string_lossy(),
                            scenario_names()
                        ));
                    }
                }
            }
            Some("--workers") => match count(&mut args, "--workers") {
                Ok(value) => parsed.workers = Some(value),
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--items") => match count(&mut args, "--items") {
                Ok(value) => parsed.items = value,
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--chunk") => match count(&mut args, "--chunk") {
                Ok(value) => parsed.chunk = value,
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--iterations") => match count(&mut args, "--iterations") {
                Ok(value) => parsed.iterations = value,
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--warmup") => match count(&mut args, "--warmup") {
                Ok(value) => parsed.warmup = value,
                Err(message) => return Invocation::BadUsage(message),
            },
            Some(other) => {
                return Invocation::BadUsage(format!("`bench` has no argument `{other}`"));
            }
            None => {
                return Invocation::BadUsage(format!(
                    "`bench` has no argument `{}`",
                    arg.to_string_lossy()
                ));
            }
        }
    }

    let Some(scenario) = scenario else {
        return Invocation::BadUsage(format!(
            "`bench` needs a --scenario (known: {})",
            scenario_names()
        ));
    };
    parsed.scenario = scenario;

    // The three counts a zero makes meaningless, refused here rather than
    // producing a run whose every sample is the cost of doing nothing. Zero
    // workers and zero warm-up are both real answers and are not in this list.
    for (value, flag, why) in [
        (
            parsed.items,
            "--items",
            "there would be nothing to run over",
        ),
        (
            parsed.iterations,
            "--iterations",
            "there would be nothing to time",
        ),
        (
            parsed.chunk,
            "--chunk",
            "`par_for` reads a chunk of zero as one, so ask for the length you mean",
        ),
    ] {
        if value == 0 {
            return Invocation::BadUsage(format!("`{flag}` cannot be zero: {why}"));
        }
    }

    Invocation::Command(Command::Bench(parsed))
}

/// The next argument as a count, for the five numbers `bench` takes.
///
/// Separate from [`index`] because the two say different things when they
/// refuse: an index is a position in a file's node list and a count is a size,
/// and a message naming the wrong one sends a reader to the wrong flag.
fn count(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<usize, String> {
    let Some(value) = args.next() else {
        return Err(format!("{flag} needs a count"));
    };
    value
        .to_str()
        .and_then(|text| text.parse::<usize>().ok())
        .ok_or_else(|| {
            format!(
                "`{flag}` expects a whole number; got `{}`",
                value.to_string_lossy()
            )
        })
}

/// The next argument as a zero-based index, for `--node` and `--primitive`.
fn index(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<usize, String> {
    let Some(value) = args.next() else {
        return Err(format!("{flag} needs an index"));
    };
    value
        .to_str()
        .and_then(|text| text.parse::<usize>().ok())
        .ok_or_else(|| {
            format!(
                "`{flag}` expects a zero-based index; got `{}`",
                value.to_string_lossy()
            )
        })
}

/// `L,R,T,B`, in the order [`NineSlice::new`] takes.
///
/// Comma-separated rather than four flags or four positionals because the four
/// numbers are one value: three of them without the fourth is not a nine-slice,
/// and the parser should say so in one message instead of three.
fn parse_insets(raw: &str) -> Result<NineSlice, String> {
    let mut parts = raw.split(',');
    let mut edges = [0u32; 4];
    for edge in &mut edges {
        let Some(Ok(value)) = parts.next().map(str::parse::<u32>) else {
            return Err(insets_syntax(raw));
        };
        *edge = value;
    }
    if parts.next().is_some() {
        return Err(insets_syntax(raw));
    }
    // Whether the insets *fit* is checked once the frame size is known, which
    // is after the first PNG has been decoded. Here there is nothing to check
    // them against.
    Ok(NineSlice::new(edges[0], edges[1], edges[2], edges[3]))
}

fn insets_syntax(raw: &str) -> String {
    format!("`--nine` expects L,R,T,B, e.g. 4,4,4,4; got `{raw}`")
}

/// Whether `name` is a frame or clip name a `.crpix` can spell back.
///
/// Checked because `trace` writes the name into the file verbatim and does not
/// look at it: a name with a space in it becomes two tokens in a clip's frame
/// list, a `:` ends the name early in a `frame …:` line, and a `#` opens a
/// comment. Each produces a file the format's own parser refuses or, worse,
/// reads as something else — so the refusal belongs here, before anything is
/// written. The clip keywords are refused for the same reason: the parser reads
/// `loop`, `reverse`, `pingpong` and `@` as flags, not frame names.
pub fn check_sheet_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("it is empty");
    }
    if name.chars().any(char::is_whitespace) {
        return Err("a clip lists its frames separated by spaces, so a name cannot contain one");
    }
    if name.contains(':') {
        return Err("`:` is what ends a name in the format");
    }
    if name.contains('#') {
        return Err("`#` opens a comment, so the rest of the name would be discarded");
    }
    // A name that is exactly a clip keyword is read as that keyword when the
    // clip's frame list is parsed back: `loop`, `reverse` and `pingpong`
    // become flags and `@` becomes the hold marker. A frame named any of them
    // writes a clip that is not the frames asked for — silently, with exit 0 —
    // and a stem of exactly `@` fails the parse-back and blames the tool. Only
    // the exact token collides: the parser matches whole whitespace-separated
    // tokens, so `loop2` and `a@b` are ordinary frame names.
    if matches!(name, "loop" | "reverse" | "pingpong" | "@") {
        return Err(
            "it is a clip keyword (`loop`, `reverse`, `pingpong`, `@`) that the format reads as a flag",
        );
    }
    Ok(())
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
            vec!["crpix", "a.png", "-o", "a.crpix", "--json"],
            vec!["lod", "stats", "a.gltf", "--json"],
            vec!["lod", "gen", "a.gltf", "-o", "a.dag", "--json"],
            vec!["bench", "--scenario", "jobs", "--json"],
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

    /// The difference between "unknown target" and "wrong tool" is the
    /// difference between a typo and a signpost, so `wasm` parses and is
    /// refused downstream — see [`crate::cargo`].
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
            // `crpix` needs both halves of its invocation, and every flag that
            // takes a value needs one.
            vec!["crpix"],
            vec!["crpix", "a.png"],
            vec!["crpix", "-o", "out.crpix"],
            vec!["crpix", "a.png", "-o"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--nine"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--nine", "4,4,4"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--nine", "4,4,4,4,4"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--nine", "4,4,4,x"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--nine", "-1,0,0,0"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--sample"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--sample", "linear"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--clip"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--clip", "two words"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--clip", "a:b"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--clip", ""],
            vec!["crpix", "a.png", "-o", "out.crpix", "--hold"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--hold", "many"],
            // The library writes no `@` for a hold of 0 and the parser refuses
            // `@ 0`, so a zero would silently read back as 1. `--clip` is here
            // so this case tests the zero and not the rule below it.
            vec![
                "crpix",
                "a.png",
                "-o",
                "out.crpix",
                "--clip",
                "flap",
                "--hold",
                "0",
            ],
            // A hold with nothing to hold: `--clip` is what writes the clip.
            vec!["crpix", "a.png", "-o", "out.crpix", "--hold", "6"],
            vec!["crpix", "a.png", "-o", "out.crpix", "--nope"],
            // `lod` needs a branch and a file, `gen` needs somewhere to write,
            // and a flag that belongs to one branch is refused on the other
            // rather than ignored.
            vec!["lod"],
            vec!["lod", "frobnicate", "a.gltf"],
            vec!["lod", "stats"],
            vec!["lod", "gen"],
            vec!["lod", "gen", "a.gltf"],
            vec!["lod", "stats", "a.gltf", "b.gltf"],
            vec!["lod", "stats", "a.gltf", "-o", "out.dag"],
            vec!["lod", "stats", "a.gltf", "--force"],
            vec!["lod", "stats", "a.gltf", "--primitive", "0"],
            vec!["lod", "stats", "a.gltf", "--node"],
            vec!["lod", "stats", "a.gltf", "--node", "first"],
            vec!["lod", "stats", "a.gltf", "--node", "-1"],
            vec!["lod", "gen", "a.gltf", "-o"],
            vec!["lod", "gen", "a.gltf", "-o", "out.dag", "--primitive"],
            vec!["lod", "gen", "a.gltf", "-o", "out.dag", "--nope"],
        ] {
            assert!(
                matches!(parse_args(&args), Invocation::BadUsage(_)),
                "{args:?} should be a bad invocation"
            );
        }
    }

    /// Every option `lod` advertises reaches the struct the command reads, and
    /// the branch is what decides which of them are legal.
    #[test]
    fn lod_takes_its_branch_its_file_and_the_options_that_belong_to_it() {
        let Command::Lod(args) = command(&[
            "lod",
            "gen",
            "meshes/car.glb",
            "-o",
            "car.dag",
            "--node",
            "3",
            "--primitive",
            "1",
            "--force",
        ]) else {
            panic!("expected lod");
        };
        assert_eq!(args.action, LodAction::Gen);
        assert_eq!(args.file, PathBuf::from("meshes/car.glb"));
        assert_eq!(args.output, Some(PathBuf::from("car.dag")));
        assert_eq!(args.node, Some(3));
        assert_eq!(args.primitive, Some(1));
        assert!(args.force);

        let Command::Lod(args) = command(&["lod", "stats", "car.glb"]) else {
            panic!("expected lod");
        };
        assert_eq!(args.action, LodAction::Stats);
        assert_eq!((args.node, args.primitive, args.output), (None, None, None));
        assert!(!args.force && !args.json);

        // `preview` parses so that the command can refuse it with a reason;
        // an unknown branch does not parse at all.
        let Command::Lod(args) = command(&["lod", "preview", "car.glb"]) else {
            panic!("expected lod");
        };
        assert_eq!(args.action, LodAction::Preview);
    }

    /// `lod --help` and `lod <branch> --help` are the same text, because there
    /// is one page and a branch is not a subcommand with options of its own.
    #[test]
    fn lod_help_is_reachable_from_the_branch_as_well_as_the_command() {
        for argv in [
            vec!["lod", "--help"],
            vec!["lod", "-h"],
            vec!["lod", "stats", "--help"],
            vec!["lod", "gen", "a.gltf", "--help"],
        ] {
            assert_eq!(parse_args(&argv), Invocation::Help(LOD_USAGE), "{argv:?}");
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

    /// The scene selector, including the default: `crcbl screenshot` with no
    /// `--scene` has to keep drawing what it drew before there was one, because
    /// the golden-image suites that call it were written against that frame.
    ///
    /// Every scene in [`SCENES`] is driven through the real parser rather than
    /// through [`scene_from_name`], because the failure this guards is a name
    /// the *parser* cannot reach — a table entry the `--scene` arm never
    /// consults would pass a direct call and fail here.
    #[test]
    fn every_scene_in_the_table_is_reachable_and_the_default_is_still_the_cube() {
        use crcbl::screenshot::Scene;

        let Command::Screenshot(args) = command(&["screenshot"]) else {
            panic!("expected screenshot");
        };
        assert_eq!(args.scene, Scene::Cube, "the default frame moved");

        for &scene in SCENES {
            let name = scene_name(scene);
            let argv = vec!["screenshot", "--scene", name];
            let Command::Screenshot(args) = command(&argv) else {
                panic!("expected screenshot");
            };
            assert_eq!(args.scene, scene, "--scene {name}");
        }

        // Distinct names, because [`scene_from_name`] takes the first match: two
        // scenes sharing one word would make the second unreachable while every
        // assertion above still passed.
        let mut names: Vec<&str> = SCENES.iter().map(|&scene| scene_name(scene)).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "two scenes answer to one --scene name");

        // A scene the CLI does not know is exit 2, not a silent fall back to the
        // default — a typo in a harness would otherwise compare the cube twice
        // and report that every shader agrees.
        for argv in [
            vec!["screenshot", "--scene"],
            vec!["screenshot", "--scene", "menu"],
        ] {
            assert!(
                matches!(parse_args(&argv), Invocation::BadUsage(_)),
                "{argv:?} should be a bad invocation"
            );
        }
    }

    /// `--help` lists what `--scene` takes, and the two are the same list.
    ///
    /// The usage text is a literal — `concat!` takes literals and a `&'static
    /// str` const is not one — so this is what stops it drifting from
    /// [`SCENES`], exactly as `the_screenshot_help_names_the_real_size_cap`
    /// stops it drifting from the size cap.
    #[test]
    fn the_screenshot_help_names_every_scene_it_will_draw() {
        for &scene in SCENES {
            let name = scene_name(scene);
            assert!(
                SCREENSHOT_USAGE.contains(name),
                "`screenshot --help` does not name the scene `{name}`:\n\
                 {SCREENSHOT_USAGE}"
            );
        }
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

    /// Every option `crpix` advertises reaches the struct the command reads,
    /// and the inputs keep the order they were typed in — which is the frame
    /// order of the sheet, so it is the one thing here that is not cosmetic.
    #[test]
    fn crpix_takes_its_inputs_in_order_and_all_of_its_options() {
        let Command::Crpix(args) = command(&[
            "crpix",
            "up.png",
            "mid.png",
            "down.png",
            "-o",
            "bird.crpix",
            "--force",
            "--nine",
            "1,2,3,4",
            "--sample",
            "smooth",
            "--clip",
            "flap",
            "--hold",
            "6",
        ]) else {
            panic!("expected crpix");
        };
        assert_eq!(
            args.inputs,
            ["up.png", "mid.png", "down.png"].map(PathBuf::from)
        );
        assert_eq!(args.output, PathBuf::from("bird.crpix"));
        assert!(args.force);
        // Left, right, top, bottom — the order `NineSlice::new` takes and the
        // order the generated `nine:` line is written in.
        assert_eq!(args.nine, Some(NineSlice::new(1, 2, 3, 4)));
        assert_eq!(args.sample, SampleMode::Smooth);
        assert_eq!(args.clip.as_deref(), Some("flap"));
        assert_eq!(args.hold, 6);
    }

    /// The defaults, stated as a test because they are the documented surface:
    /// no nine-slice, `pixel`, no clip, a hold of one tick, and no overwrite.
    #[test]
    fn crpix_defaults_to_a_plain_sheet_that_will_not_overwrite() {
        let Command::Crpix(args) = command(&["crpix", "a.png", "-o", "a.crpix"]) else {
            panic!("expected crpix");
        };
        assert_eq!(args.nine, None);
        assert_eq!(args.sample, SampleMode::Pixel);
        assert_eq!(args.clip, None);
        assert_eq!(args.hold, 1);
        assert!(!args.force);
        assert!(!args.json);
    }

    /// An input path is a path: it may be anything a filesystem accepts, and
    /// a flag-looking one after `-o` is that flag's value, not a new flag.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_input_or_output_reaches_the_command() {
        use std::os::unix::ffi::OsStringExt;

        let weird = || OsString::from_vec(b"/tmp/fr\xffme.png".to_vec());
        let Invocation::Command(Command::Crpix(args)) = parse(vec![
            OsString::from("crpix"),
            weird(),
            OsString::from("-o"),
            weird(),
        ]) else {
            panic!("a non-UTF-8 input and output are a usable invocation");
        };
        assert_eq!(args.inputs, vec![PathBuf::from(weird())]);
        assert_eq!(args.output, PathBuf::from(weird()));
    }

    /// The `bench` defaults, stated as a test because they are the documented
    /// surface — and because a sweep of `--chunk` only means something against
    /// a starting point that is written down.
    #[test]
    fn bench_defaults_to_the_pass_it_stands_in_for() {
        let Command::Bench(args) = command(&["bench", "--scenario", "jobs"]) else {
            panic!("expected bench");
        };
        assert_eq!(args.scenario, BenchScenario::Jobs);
        assert_eq!(args.items, DEFAULT_BENCH_ITEMS);
        assert_eq!(args.chunk, DEFAULT_BENCH_CHUNK);
        assert_eq!(args.iterations, DEFAULT_BENCH_ITERATIONS);
        assert_eq!(args.warmup, DEFAULT_BENCH_WARMUP);
        // `None`, not a number: the pool's own rule is what picks it, and the
        // parser must not bake a worker count into the invocation.
        assert_eq!(args.workers, None);
        assert!(!args.json);

        let Command::Bench(args) = command(&[
            "bench",
            "--scenario",
            "jobs",
            "--workers",
            "0",
            "--items",
            "64",
            "--chunk",
            "8",
            "--iterations",
            "3",
            "--warmup",
            "0",
        ]) else {
            panic!("expected bench");
        };
        // Zero workers and zero warm-up are answers, not typos.
        assert_eq!(args.workers, Some(0));
        assert_eq!(args.warmup, 0);
        assert_eq!((args.items, args.chunk, args.iterations), (64, 8, 3));
    }

    /// A scenario that answers to nothing is refused **by name**, with the ones
    /// that do exist listed — and omitting `--scenario` is refused the same way
    /// rather than running whichever happens to be first.
    #[test]
    fn bench_refuses_an_unknown_scenario_and_names_the_ones_that_exist() {
        let Invocation::BadUsage(message) = parse_args(&["bench", "--scenario", "frobnicate"])
        else {
            panic!("an unknown scenario is a bad invocation");
        };
        assert!(message.contains("frobnicate"), "{message}");
        assert!(message.contains("jobs"), "{message}");

        let Invocation::BadUsage(message) = parse_args(&["bench"]) else {
            panic!("a missing --scenario is a bad invocation");
        };
        assert!(message.contains("jobs"), "{message}");
    }

    /// The three counts a zero makes meaningless, each refused naming its own
    /// flag. `--workers 0` and `--warmup 0` are in the accepted list above.
    #[test]
    fn bench_refuses_the_counts_a_zero_would_empty() {
        for flag in ["--items", "--chunk", "--iterations"] {
            let Invocation::BadUsage(message) =
                parse_args(&["bench", "--scenario", "jobs", flag, "0"])
            else {
                panic!("`{flag} 0` should be a bad invocation");
            };
            assert!(message.contains(flag), "{message}");
        }
        // And a count that is not a number at all.
        assert!(matches!(
            parse_args(&["bench", "--scenario", "jobs", "--items", "lots"]),
            Invocation::BadUsage(_)
        ));
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
