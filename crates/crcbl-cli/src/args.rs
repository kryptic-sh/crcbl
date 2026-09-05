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
    import        Import a glTF and report what came out of it.
    bench         Run a fixed benchmark scenario and report its distribution.
    sim           Run the determinism harness and print its state hash.
    settings      Read or write a game's settings.toml.

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
                           area_light          a strip light and a fill strip
                                               over a dark glossy floor
                           fill_light          a point pair and a spot pair on
                                               that floor, each mirrored and
                                               each half of it a fill light
                           alpha_mask          a masked plate over a floor, and
                                               the hole in its shadow
                           specular_aa         a corrugated conductor plate
                                               beside a flat one, under one sun
                           ao                  the inside of a box, ambient only
                           ssr                 a pyramid reflected in a smooth
                                               floor
                           bloom               one very bright patch and its
                                               halo
                           aa                  a slab whose silhouette runs
                                               diagonally, resolved
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

/// `crcbl import --help`.
pub const IMPORT_USAGE: &str = "\
crcbl import — import a glTF and report what came out of it

USAGE:
    crcbl import <FILE> [OPTIONS]

Runs the asset import pipeline over one document and prints what it holds: the
meshes, the primitives across them, the materials, the images, every entry of
the nodes array, and the instances — one per node that draws a mesh.

FILE is a .gltf or .glb, and its file name has to be a legal asset key: ASCII
letters, digits, `.`, `_` and `-`. That is the rule every asset this engine
loads obeys, so that a tree which loads from a directory also loads over HTTP.

WHAT WAS SKIPPED:
    The importer reports its own skips as warnings on stderr — an extension it
    does not support, an image whose URI will not resolve, a primitive that is
    not a triangle list — and this verb installs the engine logger so they are
    on the terminal beside the counts. CRCBL_LOG sets the level; the default
    shows them.

    A skip is not a failure. A document that imported with warnings exits 0, and
    the counts include what was skipped: an image the file names but the
    directory does not have is still an image the document declares.

    This verb does not write a scene. `docs/plan/11-cli-headless.md` sketches
    `--out <DIR>`, and there is nothing for it to write — the importer produces
    an in-memory scene and this tree has no on-disk scene format — so `--out` is
    refused by name rather than ignored.

OPTIONS:
        --json    Emit one JSON object instead of human output.
    -h, --help    Print this text.";

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
    phys    `crcbl_phys`'s broadphase at scale, on one thread: building a tree
            over N spheres, refitting it after every one of them moves a tick's
            worth, and then N sphere overlaps, one per body. The three phases
            are timed and reported separately. `--ticks` repeats the movement
            so the queries run against a tree the crowd has walked away from.

OPTIONS (both scenarios):
        --scenario <NAME>    Which scenario to run. Required.
        --iterations <N>     Timed iterations — a `par_for` call for `jobs`, one
                             build, refit and query pass for `phys`.
                             Default: 200. Below 20 the run reports its maximum
                             and no percentile.
        --warmup <N>         Untimed iterations first, excluded from the
                             statistics. Default: 20.
        --json               Emit one JSON object instead of human output.
    -h, --help               Print this text.

OPTIONS (jobs):
        --workers <N>        Pool worker threads. Default: one fewer than the
                             machine's parallelism, which is what `Pool::new`
                             asks for. 0 runs every chunk on the calling thread.
        --items <N>          Items in the workload. Default: 10000.
        --chunk <N>          Items per `par_for` chunk, and the thing worth
                             sweeping. Default: 64.

OPTIONS (phys):
        --bodies <N>         Sphere colliders in the world, and therefore also
                             the number of overlap queries a pass runs.
                             Default: 2000.
        --extent <UNITS>     Side of the square arena they are placed in, in
                             whole world units. Default: 48. This is the density
                             control: the same body count in a smaller arena is
                             a denser crowd, more neighbours per query, and a
                             slower query phase — the run reports the neighbours
                             it actually found, so the two numbers can be read
                             together.
        --ticks <N>          Drift-and-refit steps run before the query phase,
                             each one tick's worth of movement. Default: 1. This
                             is the ageing control: a refit never re-picks a
                             leaf's place, so a crowd that has walked for N
                             ticks is queried through a tree still fitted to
                             where it started. The refit line times the last
                             step alone, so it is one tick's cost at every N;
                             the crowd also spreads as it walks, so read the
                             query line against the neighbour count beside it.

A flag that belongs to one scenario is refused on the other rather than
ignored.";

/// `crcbl sim --help`.
///
/// The three defaults and the tick-rate range are written here as literals,
/// because a `const &str` cannot interpolate — `concat!` takes literals. They
/// are pinned to [`DEFAULT_SIM_TICKS`], [`DEFAULT_SIM_TICK_RATE`],
/// [`DEFAULT_SIM_SEED`] and [`MAX_TICK_RATE`] by
/// `the_sim_help_names_the_real_defaults_and_the_tick_rate_cap`, which is the
/// only thing that can stop the two drifting — the same arrangement
/// [`BENCH_USAGE`] and [`SCREENSHOT_USAGE`] have.
pub const SIM_USAGE: &str = "\
crcbl sim — the determinism harness

USAGE:
    crcbl sim [OPTIONS]

Runs a headless server simulation for N ticks over a seed-generated world and
prints the world's state hash. Same input, same hash, and the tick loop is
provably deterministic; a hash that moves between two runs of one build is the
harness reporting exactly what it exists to catch.

The world comes from --seed and from nothing else. `docs/plan/11-cli-headless.md`
sketches `crcbl sim <scene> --input script.ron`, and neither half is built: this
tree has no scene file format and no RON reader, so there is nothing for a scene
argument to name and no script to replay. Both are refused rather than ignored.
There is no --hash flag either — the hash is the output.

OPTIONS:
        --ticks <N>        Ticks to simulate. Default: 1000. Zero is a legal
                           run of length zero, not an error.
        --tick-rate <HZ>   Server tick rate, 1..=1000000000. Default: 60. It
                           sets the clock's period and never the tick count.
        --seed <SEED>      World-generation seed. Default: 0.
        --json             Emit one JSON object instead of human output.
    -h, --help             Print this text.

OUTPUT:
    hash:<hex> ticks:<n> final_tick:<n>";

/// `crcbl settings --help`.
pub const SETTINGS_USAGE: &str = "\
crcbl settings — read or write a game's settings.toml

USAGE:
    crcbl settings [OPTIONS] list
    crcbl settings [OPTIONS] get <KEY>
    crcbl settings [OPTIONS] set <KEY> <VALUE>
    crcbl settings [OPTIONS] preset [<TIER>]

The file is `settings.toml` in the game's own config directory — on Linux that
is ~/.config/<APP>/settings.toml, and it is whatever the platform names
elsewhere. Every subcommand reports the path it used, so which file was read is
never a guess.

APP is the game the settings belong to. Without --app it is the package name of
the nearest Cargo.toml at or above the current directory, which is the same
project `crcbl run` and `crcbl build` work on. Outside a project, or in a
workspace root that declares no package, there is nothing to derive it from and
the command refuses rather than inventing a name.

`list` and `get` only read, and so does `preset` with no tier. `set` and
`preset <TIER>` are the only things here that write, and the only things that
create the config directory — a game's start-up never does.

WHAT list SHOWS:
    The player's settings file, and nothing else. A game's compiled-in defaults
    are layers that live in the game's own binary, so a key the game defaults
    and the player has never changed is absent here and still has a value when
    the game runs.

HOW set TYPES A VALUE:
    TOML's own value grammar decides, because the file it lands in is TOML:

        true, false            a boolean
        42, -7, 0x2a, 1_000    an integer
        1.5, 2e3, inf, nan     a float
        \"true\", \"42\"           quoted: a string of those characters
        anything else          a string of exactly the characters typed

    The type is the point. `engine.video.shadows` is read back as a boolean, and
    the string \"false\" is not one, so a value written as text there would be a
    line in the file that does nothing at all. Quote a value to force it to be
    text; a shell eats one layer of quotes, so that is `set game.name '\"42\"'`.

    A list or a table is stored as the text that was typed: this verb writes
    scalars, and `get` renders the same four kinds. A settings file may hold
    either — `list` shows them — but neither is something to write from here.

    A value starting with `-` would be read as an option, so `--` ends the
    options: `crcbl settings set game.offset -- -7`.

KEYS:
    A dotted path, `engine.video.shadows`. Each segment is ASCII letters,
    digits, `_` or `-`, which is what a TOML bare key is.

WHAT preset DOES:
    A quality tier is not a key: it is a name for a set of `[engine.video]`
    values, and selecting one writes every key it covers. So `preset low` and
    the `set` lines for those keys are the same edit to the same file, and
    nothing stores the word `low` anywhere.

    Bare `preset` prints the tier the file is on, which is derived from those
    keys rather than remembered — `custom` is what it says when they are not
    any one tier's set, which is a fresh file, a hand-edited one, and one key
    moved off a tier alike. The engine owns the list of tiers, so a name it
    does not know is refused with the names it does know.

    A preset written from here reaches the game at its next start-up, and the
    command says so: this process holds no renderer to show a setting in, so
    there is nothing for the write to change until the game reads the file.

EXIT CODES:
    0  the command answered, `get` on a key that is not set included
    1  the command failed: the file could not be read or written, `get` found
       a table or a list, which it does not render — `list` shows those — or
       `preset` was given a name that is not a quality tier
    2  the invocation was malformed

OPTIONS:
        --app <NAME>        Whose settings these are. Default: the package name
                            of the project in the current directory.
        --config-dir <DIR>  Use DIR in place of the platform's config directory,
                            so the file is DIR/<APP>/settings.toml. For a
                            sandbox, a CI job, or a second profile.
        --json              Emit one JSON object instead of human output.
    -h, --help              Print this text.";

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
    /// One glTF document, imported and counted.
    Import(ImportArgs),
    /// One fixed benchmark scenario, timed.
    Bench(BenchArgs),
    /// The determinism harness: N ticks of a seed-generated world, hashed.
    Sim(SimArgs),
    /// A game's `settings.toml`, read or written.
    Settings(SettingsArgs),
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
            Self::Import(_) => "import",
            // The scenario is a field of its own, for the reason above.
            Self::Bench(_) => "bench",
            Self::Sim(_) => "sim",
            // The branch is a field of its own, for the reason above.
            Self::Settings(_) => "settings",
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
            Self::Import(args) => args.json,
            Self::Bench(args) => args.json,
            Self::Sim(args) => args.json,
            Self::Settings(args) => args.json,
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

/// `crcbl import`.
///
/// There is no output directory: `docs/plan/11-cli-headless.md` sketches
/// `--out <dir>` and there is nothing for it to write, because the importer
/// produces an in-memory scene and this tree has no on-disk scene format. See
/// [`IMPORT_USAGE`], which says so where a user reads it, and `parse_import`,
/// which refuses `--out` by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportArgs {
    /// The glTF to read.
    pub file: PathBuf,
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
    /// `crcbl_phys`'s broadphase — build, refit and overlap — on one thread.
    ///
    /// The second one because `docs/plan/ROADMAP.md`'s P8 proposes adopting
    /// that broadphase onto `crcbl-jobs`, and nothing has ever timed it. See
    /// `crate::bench::phys` for what the fixture is and why density is a
    /// parameter of it rather than a detail.
    Phys,
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
            Self::Phys => "phys",
        }
    }
}

/// Every scenario, for the name lookup and the rejection message.
const SCENARIOS: &[BenchScenario] = &[BenchScenario::Jobs, BenchScenario::Phys];

/// Which scenario each of `bench`'s per-scenario flags belongs to.
///
/// A table rather than a `match` in the parser so the refusal message can name
/// the owner, and so the two halves of the help text have one list behind them.
/// `--iterations`, `--warmup` and `--json` are absent because every scenario
/// reads them.
const SCENARIO_FLAGS: &[(&str, BenchScenario)] = &[
    ("--workers", BenchScenario::Jobs),
    ("--items", BenchScenario::Jobs),
    ("--chunk", BenchScenario::Jobs),
    ("--bodies", BenchScenario::Phys),
    ("--extent", BenchScenario::Phys),
    ("--ticks", BenchScenario::Phys),
];

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

/// Sphere colliders the `phys` scenario builds a tree over when `--bodies` is
/// not given.
///
/// Not [`DEFAULT_BENCH_ITEMS`]'s ten thousand, and the difference is the point:
/// one `phys` iteration is `bodies` inserts, `bodies` moves and `bodies` overlap
/// queries, and the run's answer guard is an `O(bodies²)` scan on top. Two
/// thousand is the largest round number whose default invocation still finishes
/// in a couple of seconds in a checked build — sweep upwards from it on an
/// optimised one.
pub const DEFAULT_BENCH_BODIES: usize = 2_000;

/// Side of the square arena, in whole world units, when `--extent` is not given.
///
/// Chosen so the default invocation reports a handful of neighbours per query —
/// the sparse end of what `apps/horde` sees, and the density its steering pass
/// was written against. `docs/backlog.md` is explicit that a body count without
/// a density is not a number anybody can use, so this has a default only so that
/// `--bodies` alone means something; a run that cares sets both.
pub const DEFAULT_BENCH_EXTENT: usize = 48;

/// Drift-and-refit steps the `phys` scenario runs before its query phase when
/// `--ticks` is not given.
///
/// One, because that is the run the scenario reported before the flag existed
/// and a default that moved would silently reprice every number already
/// recorded against it. It is the *uninteresting* value: the tree a one-tick
/// run queries is as tight as the build left it, and `docs/backlog.md`'s
/// question — what a refit-only tree costs once its elements have travelled —
/// only has an answer above it.
pub const DEFAULT_BENCH_TICKS: usize = 1;

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
    /// Colliders the `phys` scenario builds, moves and queries. Never zero.
    pub bodies: usize,
    /// Side of the square arena those colliders are placed in, in whole world
    /// units. Never zero: an arena of no extent stacks the whole crowd on one
    /// point, which the parser refuses.
    pub extent: usize,
    /// Drift-and-refit steps the `phys` scenario ages its crowd through before
    /// querying it. Never zero, which would leave the refit phase timing a
    /// crowd that had not moved.
    pub ticks: usize,
    /// Timed calls. Never zero, for the same reason.
    pub iterations: usize,
    /// Untimed calls first, excluded from everything reported.
    pub warmup: usize,
    /// Machine-readable output.
    pub json: bool,
}

/// The highest tick rate whose period is at least one nanosecond.
///
/// `1_000_000_000 / tick_rate` is integer division: above this it truncates to
/// zero, and `FrameClock::with_period` refuses a zero period. A zero rate used
/// to parse cleanly and then divide by zero computing the period, aborting with
/// exit 101 instead of the documented exit 2. Rejecting the whole range here
/// makes both ends a bad invocation rather than an assert.
pub const MAX_TICK_RATE: u32 = 1_000_000_000;

/// Ticks `crcbl sim` runs when `--ticks` is not given.
pub const DEFAULT_SIM_TICKS: u64 = 1_000;

/// The tick rate `crcbl sim` clocks at when `--tick-rate` is not given.
///
/// The server's own rate, so the default run is the loop a game actually has.
pub const DEFAULT_SIM_TICK_RATE: u32 = 60;

/// The world seed `crcbl sim` builds from when `--seed` is not given.
pub const DEFAULT_SIM_SEED: u64 = 0;

/// `crcbl sim`.
///
/// There is no scene and no input script: `docs/plan/11-cli-headless.md`
/// sketches both and this tree has neither a scene file format nor a RON
/// reader, so the world is generated from [`seed`](Self::seed) alone. See
/// [`SIM_USAGE`], which says so where a user reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimArgs {
    /// Ticks to run. Zero is a legal run of length zero.
    pub ticks: u64,
    /// Clock rate in hertz. Always in `1..=`[`MAX_TICK_RATE`]; the parser
    /// refuses everything else, because a zero period is an assert several
    /// layers down rather than a message.
    pub tick_rate: u32,
    /// World-generation seed.
    pub seed: u64,
    /// Machine-readable output.
    pub json: bool,
}

/// Which half of `crcbl settings` was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    /// Print the whole file.
    List,
    /// Print one dotted key's value.
    Get,
    /// Write one dotted key's value and persist the file.
    Set,
    /// Print the quality tier the file is on, or write one.
    ///
    /// A tier is not a key — it is a name for a set of `[engine.video]` values
    /// — so it is its own branch rather than a key `set` could write. See
    /// [`crcbl::settings::presets`], which owns both the names and the values.
    Preset,
}

impl SettingsAction {
    /// The name this branch is reported under in `--json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Get => "get",
            Self::Set => "set",
            Self::Preset => "preset",
        }
    }
}

/// `crcbl settings`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsArgs {
    /// Which branch to run.
    pub action: SettingsAction,
    /// The dotted key. Always `Some` for [`SettingsAction::Get`] and
    /// [`SettingsAction::Set`] and always `None` for
    /// [`SettingsAction::List`] and [`SettingsAction::Preset`] — the parser
    /// refuses the combinations that are neither, the way [`LodArgs::output`]
    /// is refused on the branches that write nothing.
    pub key: Option<String>,
    /// The value to write, as it was typed. Always `Some` for
    /// [`SettingsAction::Set`] and always `None` otherwise.
    ///
    /// Text rather than a typed value: what type it lands as is
    /// `settings_cmd`'s decision and is documented in [`SETTINGS_USAGE`], and
    /// making it here would put half the rule in the parser.
    pub value: Option<String>,
    /// The quality tier to select, as it was typed. `None` on
    /// [`SettingsAction::Preset`] is the bare form, which prints the tier the
    /// file is on instead of moving it, and it is always `None` on the other
    /// three.
    ///
    /// Text rather than a
    /// [`QualityPreset`](crcbl::settings::presets::QualityPreset), for
    /// [`value`](Self::value)'s reason: which words are tiers is
    /// `crcbl::settings::presets`' knowledge, and a parser that held the list
    /// would be a second copy of it that a new column has to be added to
    /// twice.
    pub tier: Option<String>,
    /// The game whose settings these are. `None` is "derive it from the project
    /// in the current directory", which is a filesystem question and so is left
    /// to the command.
    pub app: Option<String>,
    /// Stands in for the platform's config directory, so the file is
    /// `<config_dir>/<app>/settings.toml`.
    ///
    /// `None` is the platform's own answer. This exists because the platform's
    /// answer is not redirectable everywhere — `dirs` reads `XDG_CONFIG_HOME`
    /// on Linux and a Windows known folder on Windows — so a test, a container
    /// or a CI job that must not touch the developer's real `~/.config` has no
    /// portable way to say so through the environment.
    pub config_dir: Option<PathBuf>,
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
        Some("import") => parse_import(args),
        Some("bench") => parse_bench(args),
        Some("sim") => parse_sim(args),
        Some("settings") => parse_settings(args),
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
        Scene::AreaLight,
        Scene::FillLight,
        Scene::AlphaMask,
        Scene::SpecularAa,
        Scene::Ao,
        Scene::Ssr,
        Scene::Bloom,
        Scene::Aa,
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
        Scene::AreaLight => "area_light",
        Scene::FillLight => "fill_light",
        Scene::AlphaMask => "alpha_mask",
        Scene::SpecularAa => "specular_aa",
        Scene::Ao => "ao",
        Scene::Ssr => "ssr",
        Scene::Bloom => "bloom",
        Scene::Aa => "aa",
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

fn parse_import(args: impl Iterator<Item = OsString>) -> Invocation {
    let mut json = false;
    let mut file = None;

    // A `for` rather than the `while let` every other parser here uses: no arm
    // of this one reads a value of its own, so nothing needs the iterator.
    for arg in args {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(IMPORT_USAGE),
            Some("--json") => json = true,
            // Sketched by topic 11 and not built. Refused with the reason
            // rather than as an unknown option: the difference between "not
            // yet" and "typo", the same distinction `Target::Wasm`,
            // [`LodAction::Preview`] and `sim`'s scene argument are parsed to
            // make. The directory after it is never looked at, so the message
            // is about `--out` and not about a second file.
            Some("-o" | "--out" | "--output") => {
                return bad(
                    "`import` writes nothing, so it has no --out: the importer produces an \
                     in-memory scene and this tree has no on-disk scene format to write it \
                     to. `crcbl lod gen` is the one verb that writes a cooked artifact",
                );
            }
            Some(other) if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`import` has no option `{other}`"));
            }
            // The glTF. A path, so any bytes a filesystem accepts — the *asset
            // key* rules apply to the file name and are enforced by the command,
            // where a refusal can explain itself.
            Some(_) | None if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Invocation::BadUsage(format!(
                    "`import` takes one file; `{}` is a second one",
                    arg.to_string_lossy()
                ));
            }
        }
    }

    let Some(file) = file else {
        return bad("`import` needs a glTF file");
    };
    Invocation::Command(Command::Import(ImportArgs { file, json }))
}

fn parse_bench(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = BenchArgs {
        scenario: BenchScenario::default(),
        workers: None,
        items: DEFAULT_BENCH_ITEMS,
        chunk: DEFAULT_BENCH_CHUNK,
        bodies: DEFAULT_BENCH_BODIES,
        extent: DEFAULT_BENCH_EXTENT,
        ticks: DEFAULT_BENCH_TICKS,
        iterations: DEFAULT_BENCH_ITERATIONS,
        warmup: DEFAULT_BENCH_WARMUP,
        json: false,
    };
    // Separate from `parsed.scenario`'s default so that *omitting* `--scenario`
    // is refused rather than silently running whichever one happens to be first
    // in `SCENARIOS`. A benchmark that ran something other than what was asked
    // for would be the one output nobody could interpret.
    let mut scenario = None;
    // Which of the per-scenario flags were *given*, so that one belonging to the
    // other scenario is refused rather than ignored — `parse_lod`'s rule, for
    // its reason: a flag that silently does nothing teaches that it works, and
    // here it would teach that a density was applied when it was not.
    let mut given: Vec<&str> = Vec::new();

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
                Ok(value) => {
                    parsed.workers = Some(value);
                    given.push("--workers");
                }
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--items") => match count(&mut args, "--items") {
                Ok(value) => {
                    parsed.items = value;
                    given.push("--items");
                }
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--chunk") => match count(&mut args, "--chunk") {
                Ok(value) => {
                    parsed.chunk = value;
                    given.push("--chunk");
                }
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--bodies") => match count(&mut args, "--bodies") {
                Ok(value) => {
                    parsed.bodies = value;
                    given.push("--bodies");
                }
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--extent") => match count(&mut args, "--extent") {
                Ok(value) => {
                    parsed.extent = value;
                    given.push("--extent");
                }
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--ticks") => match count(&mut args, "--ticks") {
                Ok(value) => {
                    parsed.ticks = value;
                    given.push("--ticks");
                }
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

    // A flag that belongs to the scenario that was *not* asked for, refused by
    // name and pointed at the one that reads it. Driven from `SCENARIO_FLAGS`
    // rather than from `given`, so a flag the table forgot is a flag no arm
    // above can have pushed.
    for &(flag, owner) in SCENARIO_FLAGS {
        if owner != scenario && given.contains(&flag) {
            return Invocation::BadUsage(format!(
                "`{flag}` is a `{}` option and this run is `--scenario {}`",
                owner.name(),
                scenario.name()
            ));
        }
    }

    // The counts a zero makes meaningless, refused here rather than producing a
    // run whose every sample is the cost of doing nothing. Zero workers and zero
    // warm-up are both real answers and are not in this list. A zero on a flag
    // the chosen scenario does not read was already refused above, so every
    // entry here is reachable only for the scenario that owns it.
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
        (
            parsed.bodies,
            "--bodies",
            "an empty world has no tree to build and nothing to query",
        ),
        (
            parsed.extent,
            "--extent",
            "an arena of no extent stacks the whole crowd on one point, so every query \
             answers with every body",
        ),
        (
            parsed.ticks,
            "--ticks",
            "a crowd that never moves gives the refit phase nothing to refit, so its \
             timing would be the cost of setting every body back where it already was",
        ),
    ] {
        if value == 0 {
            return Invocation::BadUsage(format!("`{flag}` cannot be zero: {why}"));
        }
    }

    Invocation::Command(Command::Bench(parsed))
}

fn parse_sim(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut parsed = SimArgs {
        ticks: DEFAULT_SIM_TICKS,
        tick_rate: DEFAULT_SIM_TICK_RATE,
        seed: DEFAULT_SIM_SEED,
        json: false,
    };

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(SIM_USAGE),
            Some("--json") => parsed.json = true,
            Some("--ticks") => match whole(&mut args, "--ticks") {
                Ok(value) => parsed.ticks = value,
                Err(message) => return Invocation::BadUsage(message),
            },
            // The range is enforced at parse time, at both ends: zero divides
            // by zero computing the period and anything above `MAX_TICK_RATE`
            // truncates the period to zero nanoseconds, and `FrameClock::
            // with_period` asserts on the result. Exit 2 with a message beats
            // exit 101 with a backtrace.
            Some("--tick-rate") => match whole(&mut args, "--tick-rate") {
                Ok(value) => {
                    let rate = u32::try_from(value)
                        .ok()
                        .filter(|rate| (1..=MAX_TICK_RATE).contains(rate));
                    match rate {
                        Some(rate) => parsed.tick_rate = rate,
                        None => {
                            return Invocation::BadUsage(format!(
                                "`--tick-rate` expects a rate in 1..={MAX_TICK_RATE}; got `{value}`"
                            ));
                        }
                    }
                }
                Err(message) => return Invocation::BadUsage(message),
            },
            Some("--seed") => match whole(&mut args, "--seed") {
                Ok(value) => parsed.seed = value,
                Err(message) => return Invocation::BadUsage(message),
            },
            Some(other) if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`sim` has no option `{other}`"));
            }
            // A positional, which topic 11 sketches as the scene to simulate.
            // Refused with the reason rather than as an unknown option: the
            // difference between "not yet" and "typo", the same distinction
            // `Target::Wasm` and `LodAction::Preview` are parsed to make.
            _ => {
                return Invocation::BadUsage(format!(
                    "`sim` takes no scene: the world is generated from --seed, because this \
                     tree has no scene file format to load `{}` from",
                    arg.to_string_lossy()
                ));
            }
        }
    }

    Invocation::Command(Command::Sim(parsed))
}

fn parse_settings(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut app = None;
    let mut config_dir = None;
    let mut json = false;
    // The subcommand and its key and value, collected as they arrive so that
    // `crcbl settings --app mygame get engine.video.shadows` and
    // `crcbl settings get engine.video.shadows --app mygame` are the same
    // invocation. Every other subcommand here accepts its flags in any
    // position and this one is not the place to break that.
    let mut positional: Vec<OsString> = Vec::new();

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(SETTINGS_USAGE),
            Some("--json") => json = true,
            // Everything after `--` is a subcommand, a key or a value, whatever
            // it looks like. A settings value can start with a `-` — a negative
            // number is the ordinary case — and there is otherwise no way to
            // type one: `crcbl settings set game.offset -- -7`. Same separator
            // `run` uses for the same reason.
            Some("--") => positional.extend(args.by_ref()),
            Some("--app") => {
                let Some(value) = args.next() else {
                    return bad("--app needs a name");
                };
                let Some(name) = value.to_str() else {
                    return not_text("settings", "name for --app", &value);
                };
                match check_app_name(name) {
                    Ok(()) => app = Some(name.to_string()),
                    Err(why) => {
                        return Invocation::BadUsage(format!(
                            "`--app {name}` is not a usable directory name: {why}"
                        ));
                    }
                }
            }
            // A path, so it stays an `OsString` all the way to `PathBuf`.
            Some("--config-dir") => match args.next() {
                Some(value) if !value.is_empty() => config_dir = Some(PathBuf::from(value)),
                _ => return bad("--config-dir needs a directory"),
            },
            Some(other) if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`settings` has no option `{other}`"));
            }
            _ => positional.push(arg),
        }
    }

    let Some((first, rest)) = positional.split_first() else {
        return bad("`settings` needs a subcommand (list, get, set, preset)");
    };
    let action = match first.to_str() {
        Some("list") => SettingsAction::List,
        Some("get") => SettingsAction::Get,
        Some("set") => SettingsAction::Set,
        Some("preset") => SettingsAction::Preset,
        _ => {
            return Invocation::BadUsage(format!(
                "`settings` has no subcommand `{}` (known: list, get, set, preset)",
                first.to_string_lossy()
            ));
        }
    };

    // How many arguments each branch takes, checked before any of them is
    // read: an extra one is a typo the shell split in two, not something to
    // drop silently.
    //
    // `preset` is the one branch that takes *up to* its count rather than
    // exactly it: with a tier it writes one, without a tier it prints the one
    // the file is on, and both are things a person types on purpose.
    let wanted = match action {
        SettingsAction::List => 0,
        SettingsAction::Get | SettingsAction::Preset => 1,
        SettingsAction::Set => 2,
    };
    if rest.len() > wanted {
        return Invocation::BadUsage(format!(
            "`settings {}` takes {wanted} argument(s); `{}` is one too many",
            action.name(),
            rest[wanted].to_string_lossy()
        ));
    }

    let key = match action {
        SettingsAction::List | SettingsAction::Preset => None,
        _ => {
            let Some(raw) = rest.first() else {
                return Invocation::BadUsage(format!(
                    "`settings {}` needs a key, such as `engine.video.shadows`",
                    action.name()
                ));
            };
            let Some(key) = raw.to_str() else {
                return not_text("settings", "key", raw);
            };
            if let Err(why) = check_settings_key(key) {
                return Invocation::BadUsage(format!("`{key}` is not a settings key: {why}"));
            }
            Some(key.to_string())
        }
    };

    let value = match action {
        SettingsAction::Set => {
            let Some(raw) = rest.get(1) else {
                return Invocation::BadUsage(format!(
                    "`settings set {}` needs a value",
                    key.as_deref().unwrap_or_default()
                ));
            };
            // TOML is UTF-8 by definition, so a value that is not text is one
            // that could not be written to the file whatever it means.
            let Some(value) = raw.to_str() else {
                return not_text("settings", "value", raw);
            };
            Some(value.to_string())
        }
        _ => None,
    };

    let tier = match action {
        SettingsAction::Preset => match rest.first() {
            // A tier name lands in no file, so this is only the argv's own
            // encoding: a word that is not text cannot be one of the names
            // `crcbl::settings::presets` spells.
            Some(raw) => match raw.to_str() {
                Some(name) => Some(name.to_string()),
                None => return not_text("settings", "quality tier", raw),
            },
            None => None,
        },
        _ => None,
    };

    Invocation::Command(Command::Settings(SettingsArgs {
        action,
        key,
        value,
        tier,
        app,
        config_dir,
        json,
    }))
}

/// The next argument as a whole number, for the three values `sim` takes.
///
/// Separate from [`count`] because these are `u64` and not `usize`: `--seed` is
/// an opaque sixty-four-bit value and `--ticks` is a tick budget, and neither is
/// a size that has to fit in a host pointer. Parsing them as `usize` would make
/// a thirty-two-bit host refuse seeds that a sixty-four-bit one accepts, and two
/// machines disagreeing about which seeds exist is precisely what a determinism
/// harness must not do.
fn whole(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<u64, String> {
    let Some(value) = args.next() else {
        return Err(format!("{flag} needs a number"));
    };
    value
        .to_str()
        .and_then(|text| text.parse::<u64>().ok())
        .ok_or_else(|| {
            format!(
                "`{flag}` expects a whole number; got `{}`",
                value.to_string_lossy()
            )
        })
}

/// The next argument as a count, for the numbers `bench` takes.
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

/// Whether `name` is safe to use as the config directory of a game.
///
/// It becomes a path component under the platform's config directory, so this
/// is a boundary check and not a style one: `..`, a separator, or a drive
/// prefix would put `settings.toml` somewhere the user did not name. Restricted
/// to the characters a Cargo package name already allows, because the default
/// comes from one — see [`SettingsArgs::app`].
pub fn check_app_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("it is empty");
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
    {
        return Err("only letters, digits, `-` and `_` are allowed");
    }
    if name.starts_with('-') {
        return Err("it would be read as an option");
    }
    Ok(())
}

/// Whether `key` is a dotted settings key this CLI will read and write.
///
/// Each segment has to be a TOML *bare* key, so that a key typed here is
/// spelled in `settings.toml` exactly as it was typed and a person editing the
/// file by hand meets no quoting. An empty segment — `a..b`, a leading or a
/// trailing dot — is refused rather than written: `crcbl_store`'s dotted
/// navigation splits on `.` and would create a table whose name is the empty
/// string, which nothing could then read back.
fn check_settings_key(key: &str) -> Result<(), &'static str> {
    if key.is_empty() {
        return Err("it is empty");
    }
    for segment in key.split('.') {
        if segment.is_empty() {
            return Err("a dot has nothing on one side of it");
        }
        if !segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        }) {
            return Err("a segment is ASCII letters, digits, `_` and `-`");
        }
    }
    Ok(())
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

    use std::path::Path;

    use crcbl_store::settings::SETTINGS_FILE;

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
            vec!["import", "a.gltf", "--json"],
            vec!["bench", "--scenario", "jobs", "--json"],
            vec!["bench", "--scenario", "phys", "--json"],
            vec!["sim", "--json"],
            vec!["settings", "--app", "g", "list", "--json"],
            vec!["settings", "--app", "g", "get", "a.b", "--json"],
            vec!["settings", "--app", "g", "set", "a.b", "c", "--json"],
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

    /// The one positional reaches the struct, `--json` is the only option, and
    /// the path is taken verbatim — the asset-key rules are the command's, so a
    /// name the parser cannot judge still parses.
    #[test]
    fn import_takes_one_file_and_the_json_flag() {
        let Command::Import(args) = command(&["import", "meshes/car.glb"]) else {
            panic!("expected import");
        };
        assert_eq!(args.file, PathBuf::from("meshes/car.glb"));
        assert!(!args.json);

        // Either order: the flag is not positional.
        for argv in [
            vec!["import", "car.glb", "--json"],
            vec!["import", "--json", "car.glb"],
        ] {
            let Command::Import(args) = command(&argv) else {
                panic!("expected import for {argv:?}");
            };
            assert_eq!((args.file, args.json), (PathBuf::from("car.glb"), true));
        }
    }

    #[test]
    fn import_help_is_its_own_page() {
        for argv in [vec!["import", "--help"], vec!["import", "-h"]] {
            assert_eq!(
                parse_args(&argv),
                Invocation::Help(IMPORT_USAGE),
                "{argv:?}"
            );
        }
    }

    /// The file is required, a second one is refused, and an option `import`
    /// does not have is refused as an option rather than swallowed as a path.
    #[test]
    fn import_refuses_a_malformed_invocation() {
        let Invocation::BadUsage(message) = parse_args(&["import"]) else {
            panic!("a missing file should be a bad invocation");
        };
        assert!(message.contains("needs a glTF file"), "{message}");

        let Invocation::BadUsage(message) = parse_args(&["import", "a.gltf", "b.gltf"]) else {
            panic!("two files should be a bad invocation");
        };
        assert!(message.contains("b.gltf"), "{message}");

        let Invocation::BadUsage(message) = parse_args(&["import", "a.gltf", "--frobnicate"])
        else {
            panic!("an unknown option should be a bad invocation");
        };
        assert!(message.contains("--frobnicate"), "{message}");
    }

    /// `--out` is refused **with the reason**, not as an unknown option:
    /// `docs/plan/11-cli-headless.md` sketches it and there is nothing for it to
    /// write. The difference is what tells a reader "not built" from "typo".
    #[test]
    fn import_refuses_the_output_directory_it_cannot_write() {
        for argv in [
            vec!["import", "car.glb", "--out", "cooked/"],
            vec!["import", "car.glb", "-o", "cooked/"],
            vec!["import", "car.glb", "--output", "cooked/"],
        ] {
            let Invocation::BadUsage(message) = parse_args(&argv) else {
                panic!("{argv:?} should be a bad invocation");
            };
            assert!(
                message.contains("--out") && message.contains("no on-disk scene format"),
                "{argv:?} must be refused by name and with the reason: {message}"
            );
            assert!(
                !message.contains("cooked/"),
                "the refusal is about --out, not about the directory after it: {message}"
            );
        }
    }

    /// The help says the two things a reader would otherwise have to find out by
    /// running it: `--out` is refused, and what was skipped arrives as warnings.
    #[test]
    fn the_import_help_says_what_it_does_not_do() {
        for value in ["--out", "warnings", "CRCBL_LOG"] {
            assert!(
                IMPORT_USAGE.contains(value),
                "`import --help` does not name `{value}`:\n{IMPORT_USAGE}"
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
        for scenario in SCENARIOS {
            assert!(message.contains(scenario.name()), "{message}");
        }
    }

    /// **Every scenario in the table is reachable, answers to a distinct name,
    /// and is named in the help.**
    ///
    /// Driven through the real parser rather than through [`scenario_from_name`]
    /// for the reason
    /// `every_scene_in_the_table_is_reachable_and_the_default_is_still_the_cube`
    /// gives: a table entry the `--scenario` arm never consults would pass a
    /// direct call and fail here. The help check is what stops [`BENCH_USAGE`] —
    /// a literal, because `concat!` takes literals — drifting from
    /// [`SCENARIOS`], and it is the same guard the `--scene` list has.
    #[test]
    fn every_bench_scenario_is_reachable_distinctly_named_and_documented() {
        for &scenario in SCENARIOS {
            let name = scenario.name();
            let Command::Bench(args) = command(&["bench", "--scenario", name]) else {
                panic!("expected bench");
            };
            assert_eq!(args.scenario, scenario, "--scenario {name}");
            assert!(
                BENCH_USAGE.contains(name),
                "`bench --help` does not name the scenario `{name}`:\n{BENCH_USAGE}"
            );
        }

        // Distinct names, because [`scenario_from_name`] takes the first match:
        // two scenarios sharing one word would make the second unreachable
        // while every assertion above still passed.
        let mut names: Vec<&str> = SCENARIOS.iter().map(|&s| s.name()).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "two scenarios answer to one --scenario");
    }

    /// The `phys` defaults, stated as a test for
    /// `bench_defaults_to_the_pass_it_stands_in_for`'s reason — and because
    /// `docs/backlog.md` is explicit that a body count without a density is not
    /// a number anybody can use, so the density the default run reports at has
    /// to be written down somewhere that fails when it moves.
    #[test]
    fn bench_phys_defaults_to_a_crowd_at_a_stated_density() {
        let Command::Bench(args) = command(&["bench", "--scenario", "phys"]) else {
            panic!("expected bench");
        };
        assert_eq!(args.scenario, BenchScenario::Phys);
        assert_eq!(args.bodies, DEFAULT_BENCH_BODIES);
        assert_eq!(args.extent, DEFAULT_BENCH_EXTENT);
        // One tick, which is the run the scenario reported before `--ticks`
        // existed: every number already recorded against a default invocation
        // is a number this default still produces.
        assert_eq!(args.ticks, DEFAULT_BENCH_TICKS);
        // Shared with `jobs` rather than twinned, so a sweep of one scenario's
        // sample count is a sweep of the other's.
        assert_eq!(args.iterations, DEFAULT_BENCH_ITERATIONS);
        assert_eq!(args.warmup, DEFAULT_BENCH_WARMUP);

        let Command::Bench(args) = command(&[
            "bench",
            "--scenario",
            "phys",
            "--bodies",
            "500",
            "--extent",
            "12",
            "--ticks",
            "64",
            "--iterations",
            "3",
            "--warmup",
            "0",
        ]) else {
            panic!("expected bench");
        };
        assert_eq!((args.bodies, args.extent, args.ticks), (500, 12, 64));
        assert_eq!((args.iterations, args.warmup), (3, 0));

        // The help quotes each default as a literal, so it is pinned to the
        // constant it describes — `the_screenshot_help_names_the_real_size_cap`
        // and for the same reason: `BENCH_USAGE` is a literal, because
        // `concat!` takes literals, so nothing else can stop the two drifting.
        //
        // Searched inside the `phys` section rather than the whole page, and
        // for the whole `Default: N.` phrase: a bare `48` or `64` appears in the
        // other scenario's options too, and an assertion that a number is
        // *somewhere* on the page passes for a default that moved.
        let phys_options = BENCH_USAGE
            .split_once("OPTIONS (phys):")
            .expect("the help has a phys section")
            .1;
        for value in [
            DEFAULT_BENCH_BODIES,
            DEFAULT_BENCH_EXTENT,
            DEFAULT_BENCH_TICKS,
        ] {
            assert!(
                phys_options.contains(&format!("Default: {value}.")),
                "`bench --help` does not name the default {value}:\n{phys_options}"
            );
        }

        // And a count flag with nothing after it is exit 2, not a default.
        for argv in [
            vec!["bench", "--scenario", "phys", "--bodies"],
            vec!["bench", "--scenario", "phys", "--extent"],
            vec!["bench", "--scenario", "phys", "--ticks"],
            vec!["bench", "--scenario", "phys", "--bodies", "lots"],
            vec!["bench", "--scenario", "phys", "--ticks", "many"],
        ] {
            assert!(
                matches!(parse_args(&argv), Invocation::BadUsage(_)),
                "{argv:?} should be a bad invocation"
            );
        }
    }

    /// **A flag that belongs to one scenario is refused on the other, by name,
    /// and pointed at the scenario that reads it.**
    ///
    /// `parse_lod`'s rule and its reason: a flag that silently does nothing
    /// teaches that it works. Here it would teach that a density was applied to
    /// a `jobs` run, or that a worker count changed a `phys` one — and the run
    /// would print a plausible distribution either way.
    ///
    /// Driven from [`SCENARIO_FLAGS`] so a flag added to the table without an
    /// arm to accept it fails here too.
    #[test]
    fn a_bench_flag_is_refused_on_the_scenario_that_does_not_read_it() {
        for &(flag, owner) in SCENARIO_FLAGS {
            for &scenario in SCENARIOS {
                let argv = vec!["bench", "--scenario", scenario.name(), flag, "1"];
                if scenario == owner {
                    assert!(
                        matches!(parse_args(&argv), Invocation::Command(_)),
                        "{argv:?} is the scenario that owns {flag}"
                    );
                    continue;
                }
                let Invocation::BadUsage(message) = parse_args(&argv) else {
                    panic!("{argv:?} should be a bad invocation");
                };
                assert!(message.contains(flag), "{message}");
                assert!(message.contains(owner.name()), "{message}");
                assert!(message.contains(scenario.name()), "{message}");
            }
        }
    }

    /// Every count a zero makes meaningless, each refused naming its own flag.
    /// `--workers 0` and `--warmup 0` are in the accepted list above.
    #[test]
    fn bench_refuses_the_counts_a_zero_would_empty() {
        for (scenario, flag) in [
            ("jobs", "--items"),
            ("jobs", "--chunk"),
            ("jobs", "--iterations"),
            ("phys", "--bodies"),
            ("phys", "--extent"),
            ("phys", "--ticks"),
            ("phys", "--iterations"),
        ] {
            let Invocation::BadUsage(message) =
                parse_args(&["bench", "--scenario", scenario, flag, "0"])
            else {
                panic!("`--scenario {scenario} {flag} 0` should be a bad invocation");
            };
            assert!(message.contains(flag), "{message}");
        }
        // And a count that is not a number at all.
        assert!(matches!(
            parse_args(&["bench", "--scenario", "jobs", "--items", "lots"]),
            Invocation::BadUsage(_)
        ));
    }

    /// `sim`'s defaults, and the range its tick rate is held to.
    ///
    /// The defaults are pinned because they are the invocation every recorded
    /// hash was produced by: a default that moved would silently reprice every
    /// `hash:… ticks:1000` anybody has written down.
    #[test]
    fn sim_defaults_to_a_thousand_ticks_at_the_server_rate() {
        let Command::Sim(args) = command(&["sim"]) else {
            panic!("expected sim");
        };
        assert_eq!(args.ticks, DEFAULT_SIM_TICKS);
        assert_eq!(args.tick_rate, DEFAULT_SIM_TICK_RATE);
        assert_eq!(args.seed, DEFAULT_SIM_SEED);
        assert!(!args.json);

        let Command::Sim(args) = command(&[
            "sim",
            "--ticks",
            "7",
            "--tick-rate",
            "240",
            "--seed",
            "18446744073709551615",
        ]) else {
            panic!("expected sim");
        };
        // A seed no `usize` is guaranteed to hold, taken whole — see [`whole`].
        assert_eq!((args.ticks, args.tick_rate, args.seed), (7, 240, u64::MAX));
    }

    /// Both ends of the tick-rate range, refused at parse time.
    ///
    /// Zero used to divide by zero computing the period and abort with exit 101
    /// rather than the contracted exit 2; one above [`MAX_TICK_RATE`] truncates
    /// the same division to a zero-nanosecond period, which `FrameClock::
    /// with_period` asserts on. Both are bad invocations here.
    #[test]
    fn sim_refuses_a_tick_rate_that_has_no_period() {
        for rate in ["0", "1000000001", "4294967296", "-1", "banana"] {
            assert!(
                matches!(
                    parse_args(&["sim", "--tick-rate", rate]),
                    Invocation::BadUsage(_)
                ),
                "`--tick-rate {rate}` should be a bad invocation"
            );
        }
        for rate in ["1", "1000000000"] {
            let Command::Sim(args) = command(&["sim", "--tick-rate", rate]) else {
                panic!("`--tick-rate {rate}` is inside the range");
            };
            assert_eq!(args.tick_rate.to_string(), rate);
        }
        // A flag with nothing after it is exit 2, not a default.
        for argv in [
            vec!["sim", "--ticks"],
            vec!["sim", "--tick-rate"],
            vec!["sim", "--seed"],
            vec!["sim", "--ticks", "banana"],
            vec!["sim", "--seed", "banana"],
        ] {
            assert!(
                matches!(parse_args(&argv), Invocation::BadUsage(_)),
                "{argv:?} should be a bad invocation"
            );
        }
    }

    /// A scene argument and `--input` are refused by name rather than ignored:
    /// `docs/plan/11-cli-headless.md` sketches both, this tree has neither a
    /// scene format nor a RON reader, and a positional silently dropped would
    /// print a hash for a world nobody asked for.
    #[test]
    fn sim_refuses_the_scene_and_the_input_script_it_does_not_have() {
        for argv in [vec!["sim", "--input", "script.ron"], vec!["sim", "--hash"]] {
            assert!(
                matches!(parse_args(&argv), Invocation::BadUsage(_)),
                "{argv:?} should be a bad invocation"
            );
        }
        // The positional says *why* rather than reporting an unknown option,
        // and it quotes what was typed so the message is about this run.
        let Invocation::BadUsage(message) = parse_args(&["sim", "towers.scene"]) else {
            panic!("a scene argument should be a bad invocation");
        };
        assert!(message.contains("towers.scene"), "{message}");
        assert!(message.contains("--seed"), "{message}");
    }

    /// The help quotes each default and the tick-rate cap as literals, so they
    /// are pinned to the constants they describe — `the_screenshot_help_names_
    /// the_real_size_cap`'s arrangement, for its reason: [`SIM_USAGE`] is a
    /// literal because `concat!` takes literals, so nothing else can stop the
    /// two drifting.
    #[test]
    fn the_sim_help_names_the_real_defaults_and_the_tick_rate_cap() {
        for value in [
            format!("Default: {DEFAULT_SIM_TICKS}."),
            format!("Default: {DEFAULT_SIM_TICK_RATE}."),
            format!("Default: {DEFAULT_SIM_SEED}."),
            format!("1..={MAX_TICK_RATE}"),
        ] {
            assert!(
                SIM_USAGE.contains(&value),
                "`sim --help` does not name `{value}`:\n{SIM_USAGE}"
            );
        }
        // And the output contract, which the docs and every consumer read.
        assert!(
            SIM_USAGE.contains("hash:<hex> ticks:<n> final_tick:<n>"),
            "{SIM_USAGE}"
        );
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
    // ── settings ────────────────────────────────────────────────────────

    fn settings(args: &[&str]) -> SettingsArgs {
        match command(args) {
            Command::Settings(parsed) => parsed,
            other => panic!("expected `settings`, got {other:?}"),
        }
    }

    /// Each branch parses to its own action, and only the branches that take a
    /// key and a value carry them.
    #[test]
    fn settings_parses_its_four_subcommands() {
        let list = settings(&["settings", "list"]);
        assert_eq!(list.action, SettingsAction::List);
        assert_eq!((list.key.as_deref(), list.value.as_deref()), (None, None));

        let get = settings(&["settings", "get", "engine.video.shadows"]);
        assert_eq!(get.action, SettingsAction::Get);
        assert_eq!(get.key.as_deref(), Some("engine.video.shadows"));
        assert_eq!(get.value, None);

        let set = settings(&["settings", "set", "engine.video.shadows", "false"]);
        assert_eq!(set.action, SettingsAction::Set);
        assert_eq!(set.key.as_deref(), Some("engine.video.shadows"));
        assert_eq!(set.value.as_deref(), Some("false"));

        let preset = settings(&["settings", "preset", "low"]);
        assert_eq!(preset.action, SettingsAction::Preset);
        assert_eq!(preset.tier.as_deref(), Some("low"));
        assert_eq!(
            (preset.key.as_deref(), preset.value.as_deref()),
            (None, None),
            "a tier is not a key and not a value"
        );
    }

    /// **`preset` is the one branch whose argument is optional**, and the
    /// parser judges neither half of it: the bare form prints the tier the file
    /// is on, and which words *are* tiers is `crcbl::settings::presets`'
    /// knowledge — a parser holding that list would be a second copy of it.
    #[test]
    fn a_preset_takes_a_tier_or_nothing_and_the_parser_judges_neither() {
        let bare = settings(&["settings", "preset"]);
        assert_eq!(bare.action, SettingsAction::Preset);
        assert_eq!(bare.tier, None);

        // Not a tier, and still parsed: the refusal is the command's, so that
        // it can name the tiers that do exist.
        let nonsense = settings(&["settings", "preset", "ultra"]);
        assert_eq!(nonsense.tier.as_deref(), Some("ultra"));

        // Every other branch leaves it alone.
        for argv in [
            vec!["settings", "list"],
            vec!["settings", "get", "a.b"],
            vec!["settings", "set", "a.b", "c"],
        ] {
            assert_eq!(settings(&argv).tier, None, "{argv:?}");
        }
    }

    /// `--app` and `--config-dir` are accepted on either side of the
    /// subcommand, because every other command here takes its flags in any
    /// position and a settings invocation typed the natural way must not be a
    /// usage error.
    #[test]
    fn settings_takes_its_options_before_or_after_the_subcommand() {
        for argv in [
            vec![
                "settings",
                "--app",
                "mygame",
                "--config-dir",
                "/tmp/c",
                "get",
                "a.b",
            ],
            vec![
                "settings",
                "get",
                "a.b",
                "--app",
                "mygame",
                "--config-dir",
                "/tmp/c",
            ],
            vec![
                "settings",
                "--app",
                "mygame",
                "get",
                "--config-dir",
                "/tmp/c",
                "a.b",
            ],
        ] {
            let parsed = settings(&argv);
            assert_eq!(parsed.app.as_deref(), Some("mygame"), "{argv:?}");
            assert_eq!(
                parsed.config_dir.as_deref(),
                Some(Path::new("/tmp/c")),
                "{argv:?}"
            );
            assert_eq!(parsed.key.as_deref(), Some("a.b"), "{argv:?}");
        }
    }

    /// A negative number is an ordinary settings value and starts with the
    /// character that opens an option, so there has to be a way to type one.
    #[test]
    fn settings_takes_a_value_that_looks_like_an_option_after_a_double_dash() {
        let parsed = settings(&["settings", "set", "game.offset", "--", "-7"]);
        assert_eq!(parsed.value.as_deref(), Some("-7"));
        // Without it, `-7` is what it looks like.
        assert!(matches!(
            parse_args(&["settings", "set", "game.offset", "-7"]),
            Invocation::BadUsage(_)
        ));
    }

    /// Every shape that is not an invocation, each for a different reason: no
    /// branch, a branch that does not exist, a missing key, a missing value, an
    /// argument too many, a key that could not be written back, and an `--app`
    /// that would put `settings.toml` somewhere nobody named.
    #[test]
    fn settings_refuses_a_malformed_invocation() {
        for argv in [
            vec!["settings"],
            vec!["settings", "frobnicate"],
            vec!["settings", "get"],
            vec!["settings", "set"],
            vec!["settings", "set", "a.b"],
            vec!["settings", "list", "a.b"],
            vec!["settings", "get", "a.b", "c"],
            vec!["settings", "set", "a.b", "c", "d"],
            vec!["settings", "preset", "low", "medium"],
            vec!["settings", "get", ""],
            vec!["settings", "get", "a..b"],
            vec!["settings", "get", ".a"],
            vec!["settings", "get", "a."],
            vec!["settings", "get", "a b"],
            vec!["settings", "get", "a/b"],
            vec!["settings", "--app", "", "list"],
            vec!["settings", "--app", "..", "list"],
            vec!["settings", "--app", "a/b", "list"],
            vec!["settings", "--app", "-x", "list"],
            vec!["settings", "--app"],
            vec!["settings", "--config-dir"],
            vec!["settings", "--config-dir", "", "list"],
            vec!["settings", "--frobnicate", "list"],
        ] {
            assert!(
                matches!(parse_args(&argv), Invocation::BadUsage(_)),
                "{argv:?} should be a bad invocation"
            );
        }
    }

    /// `..` as an app name is the one refusal that is a boundary and not a
    /// style rule: it would resolve the settings file outside the config
    /// directory the platform named.
    #[test]
    fn an_app_name_is_a_single_path_component() {
        for name in ["", "..", ".", "a/b", "a\\b", "-x", "a b", "a.b"] {
            assert!(
                check_app_name(name).is_err(),
                "`{name}` must not become a directory name"
            );
        }
        for name in ["mygame", "my-game", "my_game", "2048"] {
            assert!(check_app_name(name).is_ok(), "`{name}` is a usable name");
        }
    }

    #[test]
    fn settings_help_is_the_settings_help() {
        for argv in [
            vec!["settings", "--help"],
            vec!["settings", "-h"],
            vec!["settings", "get", "--help"],
            vec!["settings", "set", "a.b", "c", "--help"],
            vec!["settings", "preset", "--help"],
        ] {
            assert_eq!(
                parse_args(&argv),
                Invocation::Help(SETTINGS_USAGE),
                "{argv:?}"
            );
        }
    }

    /// The help names the file it reads and the escape hatch for a value that
    /// looks like an option, because both are things a user cannot discover by
    /// trying.
    #[test]
    fn the_settings_help_names_the_file_and_the_double_dash() {
        for value in [
            SETTINGS_FILE,
            "set game.offset -- -7",
            "--app",
            "--config-dir",
            // A tier is not a key, so `preset` is the one branch a person
            // cannot guess the existence of from the other three.
            "preset [<TIER>]",
        ] {
            assert!(
                SETTINGS_USAGE.contains(value),
                "`settings --help` does not name `{value}`:\n{SETTINGS_USAGE}"
            );
        }
    }
}
