//! The `crcbl` binary, run for everything that does not need a compiler.
//!
//! These run in the ordinary suite on every platform: they invoke the real
//! binary, assert the exit-code contract and the `--json` shapes, and scaffold
//! into a temporary directory.
//!
//! # The split from `cli_e2e.rs` is compile cost, not depth
//!
//! Both files are end to end — both spawn the shipped binary and judge it by
//! what a user sees. The line between them is that `cli_e2e.rs` goes on to
//! *build* the project `crcbl new` scaffolds, and building a scaffold means
//! building the whole engine behind it. Minutes, not milliseconds. So that half
//! sits behind the `cli-e2e` feature with a CI job of its own, and this half
//! stays in the run a developer does on every change, exactly as the shell
//! crate's two display-dependent suites are split from its ordinary one.
//!
//! The names would be clearer the other way around, and are not being swapped:
//! `cli_e2e` follows the `<subject>_e2e.rs` convention every gated, harness-
//! driven suite in this workspace uses, and that convention is worth more than
//! this one pair reading precisely.
//!
//! `docs/plan/12-testing.md` calls the CLI "the e2e substrate: if it can't be
//! tested without a GUI, it's built wrong". These are the tests that make that
//! claim checkable.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The engine checkout these tests run inside.
fn engine_root() -> PathBuf {
    // `crates/crcbl-cli` → the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the CLI lives two levels below the workspace root")
        .to_path_buf()
}

/// A private directory that cleans itself up.
///
/// Fifteen lines instead of `tempfile`, for the reason `crates/crcbl-cli/src/args.rs`
/// gives about `clap`: this crate has no dependencies and a test helper is not
/// the thing to break that for.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "crcbl-cli-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the real binary in `cwd`.
fn crcbl(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crcbl"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("the crcbl binary runs")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("not killed by a signal")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A hand-rolled probe for `"key":<value>` in the one-line JSON this CLI emits.
///
/// The CLI does not link a JSON parser and neither does its test; what is being
/// asserted is that a *stable, greppable* schema comes out, which is the actual
/// promise made to an agent or a CI script.
fn has_field(json: &str, key: &str, value: &str) -> bool {
    json.contains(&format!("\"{key}\":{value}"))
}

/// A path as it appears *inside* the JSON, rather than as it was typed.
///
/// The CLI escapes its strings on the way out, so a Windows path is written
/// `C:\\Users\\…\\tall.png` and a search for the path as typed finds nothing —
/// the assertion would be reporting on the separator and not on the file. This
/// escapes the needle the same way: on Unix it is the identity and the check is
/// unchanged, on Windows it is the difference between a check that can pass and
/// one that cannot.
///
/// Only the two escapes a path can plausibly trigger are applied, and the
/// assertion is what keeps that "plausibly" honest: a fixture path carrying
/// anything else JSON escapes fails here rather than silently building a needle
/// that could never match.
fn json_fragment(path: &Path) -> String {
    let raw = arg(path);
    assert!(
        !raw.chars().any(|character| character < ' '),
        "a fixture path with a control character in it needs the whole escape table: {raw:?}"
    );
    raw.replace('\\', r"\\").replace('"', "\\\"")
}

/// The whole JSON string a path is written as, quotes included, for the checks
/// that want a `"key":<value>` match rather than a substring.
fn json_string(path: &Path) -> String {
    format!("\"{}\"", json_fragment(path))
}

#[test]
fn help_and_version_exit_zero() {
    let temporary = TempDir::new("help");
    for args in [vec!["--help"], vec!["help"], vec!["--version"]] {
        let output = crcbl(temporary.path(), &args);
        assert_eq!(code(&output), 0, "{args:?}");
        assert!(!stdout(&output).is_empty(), "{args:?} printed nothing");
    }
    // Per-command help too, since a CLI whose subcommands cannot explain
    // themselves is not scriptable by anyone who has not read the source.
    for command in [
        "new",
        "run",
        "build",
        "screenshot",
        "replay",
        "crpix",
        "lod",
        "bench",
        "sim",
        "settings",
    ] {
        let output = crcbl(temporary.path(), &[command, "--help"]);
        assert_eq!(code(&output), 0, "{command} --help");
        assert!(stdout(&output).contains(command), "{command} --help");
    }
}

/// Exit 2 is "bad invocation" and nothing else may use it.
#[test]
fn a_malformed_invocation_exits_two() {
    let temporary = TempDir::new("usage");
    for args in [
        vec![],
        vec!["frobnicate"],
        vec!["new"],
        vec!["new", "1bad"],
        vec!["build", "--target", "ps5"],
        vec!["run", "stray"],
        // An absurd `--size` is a bad invocation, not a 40 GB allocation and
        // not an overflowed byte count.
        vec!["screenshot", "--size", "4000000000x4000000000"],
        vec!["screenshot", "--size", "100000x100000"],
        vec!["screenshot", "--size", "0x1080"],
        vec!["screenshot", "--size", "wat"],
        // `crpix` without an output, without an input, and with a hold that
        // nothing holds.
        vec!["crpix", "a.png"],
        vec!["crpix", "-o", "out.crpix"],
        vec!["crpix", "a.png", "-o", "out.crpix", "--sample", "linear"],
        vec!["crpix", "a.png", "-o", "out.crpix", "--nine", "4,4,4"],
        vec!["crpix", "a.png", "-o", "out.crpix", "--hold", "6"],
        // `lod` without a branch, without a file, and with a flag that belongs
        // to the other branch. None of these reaches the importer, so none of
        // them needs a file that exists.
        vec!["lod"],
        vec!["lod", "frobnicate", "a.gltf"],
        vec!["lod", "stats"],
        vec!["lod", "gen", "a.gltf"],
        vec!["lod", "stats", "a.gltf", "-o", "out.dag"],
        vec!["lod", "stats", "a.gltf", "--primitive", "0"],
        vec!["lod", "stats", "a.gltf", "--node", "first"],
        vec!["lod", "stats", "a.gltf", "b.gltf"],
        // `bench` without a scenario, with one that answers to nothing, and
        // with the counts a zero empties. None of these runs anything.
        vec!["bench"],
        vec!["bench", "--scenario", "frobnicate"],
        vec!["bench", "--scenario", "jobs", "--items", "0"],
        vec!["bench", "--scenario", "jobs", "--chunk", "0"],
        vec!["bench", "--scenario", "jobs", "--iterations", "0"],
        vec!["bench", "--scenario", "jobs", "--iterations", "lots"],
        vec!["bench", "--scenario", "phys", "--bodies", "0"],
        vec!["bench", "--scenario", "phys", "--extent", "0"],
        vec!["bench", "--scenario", "phys", "--ticks", "0"],
        // A flag that belongs to the other scenario, either way round: it is
        // refused rather than ignored, because a run that quietly dropped a
        // density would still print a plausible distribution.
        vec!["bench", "--scenario", "phys", "--workers", "2"],
        vec!["bench", "--scenario", "jobs", "--extent", "12"],
        vec!["bench", "--scenario", "jobs", "--ticks", "8"],
        // `sim`'s tick rate, at both ends and in every unparseable shape. Zero
        // used to divide by zero computing the tick period and abort with exit
        // 101 rather than the contracted exit 2; one above the cap truncates
        // the same division to a zero-nanosecond period.
        vec!["sim", "--tick-rate", "0"],
        vec!["sim", "--tick-rate", "1000000001"],
        vec!["sim", "--tick-rate", "-1"],
        vec!["sim", "--tick-rate", "banana"],
        vec!["sim", "--tick-rate"],
        vec!["sim", "--ticks", "banana"],
        vec!["sim", "--seed", "banana"],
        vec!["sim", "--nonsense"],
        // The scene and the input script topic 11 sketches and this tree does
        // not have: refused by name, not ignored.
        vec!["sim", "towers.scene"],
        vec!["sim", "--input", "script.ron"],
        vec!["sim", "--hash"],
        // `settings` without a branch, with one that answers to nothing, and
        // with a key or a value missing. None of these reaches the filesystem,
        // so none of them needs a config directory that exists.
        vec!["settings"],
        vec!["settings", "frobnicate"],
        vec!["settings", "get"],
        vec!["settings", "set", "engine.video.shadows"],
        vec!["settings", "list", "engine.video.shadows"],
        vec!["settings", "get", "engine..video"],
        vec!["settings", "--app", "..", "list"],
        vec!["settings", "--config-dir"],
    ] {
        let output = crcbl(temporary.path(), &args);
        assert_eq!(code(&output), 2, "{args:?} should be exit 2");
        assert!(
            output.stdout.is_empty(),
            "{args:?} put usage diagnostics on stdout"
        );
    }
}

#[test]
fn new_scaffolds_a_complete_project() {
    let temporary = TempDir::new("new");
    let engine = engine_root();
    let output = crcbl(
        temporary.path(),
        &[
            "new",
            "mygame",
            "--engine",
            engine.to_str().expect("a UTF-8 path"),
        ],
    );
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let root = temporary.path().join("mygame");
    for file in [
        "Cargo.toml",
        "src/main.rs",
        "README.md",
        ".gitignore",
        ".github/workflows/ci.yml",
        "scenes/.gitkeep",
    ] {
        assert!(root.join(file).exists(), "{file} was not written");
    }

    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("a manifest");
    assert!(manifest.contains("name = \"mygame\""), "{manifest}");
    assert!(
        manifest.contains("[workspace]"),
        "the project must be its own workspace root:\n{manifest}"
    );
    let main = std::fs::read_to_string(root.join("src/main.rs")).expect("a main");
    assert!(
        !main.contains("{{"),
        "an unsubstituted placeholder survived into the project"
    );
}

/// The `--json` contract: one object, `ok` first, on stdout, for success and
/// for failure alike.
#[test]
fn json_output_is_one_object_per_invocation() {
    let temporary = TempDir::new("json");
    let engine = engine_root();
    let output = crcbl(
        temporary.path(),
        &[
            "new",
            "mygame",
            "--engine",
            engine.to_str().expect("a UTF-8 path"),
            "--json",
        ],
    );
    assert_eq!(code(&output), 0);
    let json = stdout(&output);
    assert_eq!(json.lines().count(), 1, "exactly one line: {json}");
    assert!(json.starts_with(r#"{"ok":true,"command":"new""#), "{json}");
    assert!(has_field(&json, "package", r#""mygame""#), "{json}");
    assert!(json.contains(r#""files":["Cargo.toml""#), "{json}");
}

/// A browser bundle is `web/build.sh`'s, not `cargo build`'s. The refusal is
/// machine-readable, so a CI job can tell "wrong tool" from "your flag is
/// wrong" without matching prose.
///
/// This used to assert the refusal named phase P5. P5 shipped, so the assertion
/// is now the other way round: the message must **not** name a phase, because a
/// refusal whose stated reason has expired reads as a bug in the CLI.
#[test]
fn building_for_wasm_points_at_the_bundle_script() {
    let temporary = TempDir::new("wasm");
    let output = crcbl(temporary.path(), &["build", "--target", "wasm", "--json"]);
    assert_eq!(code(&output), 1, "a well-formed request that cannot be met");
    let json = stdout(&output);
    assert!(
        json.starts_with(r#"{"ok":false,"command":"build""#),
        "{json}"
    );
    assert!(has_field(&json, "use", r#""web/build.sh""#), "{json}");
    assert!(
        json.contains("web/build.sh"),
        "the human message names the script too: {json}"
    );
    assert!(
        !json.contains("P5"),
        "no expired phase in the refusal: {json}"
    );

    // Without `--json` the same refusal goes to stderr and stdout stays clean,
    // so a shell pipeline is never fed an error message as data.
    let output = crcbl(temporary.path(), &["build", "--target", "wasm"]);
    assert_eq!(code(&output), 1);
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("web/build.sh"));
}

/// The "never required" half of the no-prompt rule: with no terminal to ask,
/// the CLI fails and names the flag that makes it proceed.
#[test]
fn new_refuses_to_overwrite_without_being_asked() {
    let temporary = TempDir::new("overwrite");
    let engine = engine_root();
    let engine = engine.to_str().expect("a UTF-8 path");
    let args = ["new", "mygame", "--engine", engine];

    assert_eq!(code(&crcbl(temporary.path(), &args)), 0, "the first run");

    let second = crcbl(temporary.path(), &args);
    assert_eq!(code(&second), 1, "the second must not silently overwrite");
    let message = String::from_utf8_lossy(&second.stderr).into_owned();
    assert!(message.contains("--force"), "{message}");

    let forced = crcbl(
        temporary.path(),
        &["new", "mygame", "--engine", engine, "--force"],
    );
    assert_eq!(
        code(&forced),
        0,
        "--force is what makes the prompt optional"
    );
}

#[test]
fn new_rejects_a_directory_that_is_not_an_engine_checkout() {
    let temporary = TempDir::new("noengine");
    let output = crcbl(
        temporary.path(),
        &[
            "new",
            "mygame",
            "--engine",
            temporary.path().to_str().expect("a UTF-8 path"),
            "--json",
        ],
    );
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).starts_with(r#"{"ok":false"#));
    assert!(
        !temporary.path().join("mygame").exists(),
        "nothing may be written before the engine is resolved"
    );
}

/// `screenshot` is the one subcommand whose *output* a CI job wants to read a
/// path out of, and `--json` used to emit `{"ok":true,"command":"screenshot"}`
/// and nothing else while the human line carried the path and the size.
///
/// Whether a GPU is present decides which branch runs; both are asserted,
/// because "there is no GPU here" must still be one JSON object with `ok:false`
/// rather than a panic or a bare exit code.
#[test]
fn screenshot_json_carries_the_path_and_the_dimensions() {
    let temporary = TempDir::new("screenshot");
    let target = temporary.path().join("shot.png");
    let output = crcbl(
        temporary.path(),
        &[
            "screenshot",
            "--size",
            "32x24",
            "-o",
            target.to_str().expect("a UTF-8 path"),
            "--json",
        ],
    );
    let json = stdout(&output);
    assert_eq!(json.lines().count(), 1, "exactly one line: {json}");

    if code(&output) != 0 {
        assert_eq!(code(&output), 1, "no GPU is a failure, not a bad usage");
        assert!(
            json.starts_with(r#"{"ok":false,"command":"screenshot""#),
            "{json}"
        );
        assert!(json.contains(r#""error":"#), "{json}");
        return;
    }

    assert!(
        json.starts_with(r#"{"ok":true,"command":"screenshot""#),
        "{json}"
    );
    assert!(has_field(&json, "width", "32"), "{json}");
    assert!(has_field(&json, "height", "24"), "{json}");
    assert!(
        has_field(&json, "path", &json_string(&target)),
        "the path a CI job has to pick up is in the object: {json}"
    );
    assert!(target.is_file(), "the PNG named in the JSON exists");

    // And the PNG is a PNG: the magic bytes, not just a file that was created.
    let bytes = std::fs::read(&target).expect("the screenshot");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
}

/// `std::env::args()` panics on a non-UTF-8 argument, so every path-taking flag
/// used to abort with exit 101 rather than doing its job or failing cleanly.
///
/// The path is never created. What is under test is the *argument* surviving
/// the trip through `args()` into the command, and both invocations below
/// expect exit 1 for a path that is not there — a directory that is not a
/// checkout and a replay file that does not exist. Creating it used to be part
/// of the setup and made the test macOS-only by accident: APFS enforces UTF-8
/// in filenames and answers `EILSEQ`, so the run died in `create_dir_all`
/// before the binary was ever invoked.
#[cfg(unix)]
#[test]
fn a_non_utf8_path_argument_is_not_a_panic() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let temporary = TempDir::new("nonutf8");
    let weird = temporary.path().join(OsStr::from_bytes(b"n\xfft-utf8"));
    assert!(
        weird.to_str().is_none(),
        "the argument under test has to be non-UTF-8: {weird:?}",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_crcbl"))
        .arg("new")
        .arg("mygame")
        .arg("--engine")
        .arg(&weird)
        .arg("--json")
        .current_dir(temporary.path())
        .output()
        .expect("the crcbl binary runs");

    // Exit 1: the invocation parsed fine and the directory is simply not an
    // engine checkout. What matters is that it is not 101.
    assert_eq!(
        code(&output),
        1,
        "a non-UTF-8 --engine must reach the command, not abort the process"
    );
    assert!(stdout(&output).starts_with(r#"{"ok":false"#));

    // The same for `-o`, where the path is the whole point of the flag.
    let output = Command::new(env!("CARGO_BIN_EXE_crcbl"))
        .arg("replay")
        .arg(weird.join("missing.crpl"))
        .arg("--json")
        .current_dir(temporary.path())
        .output()
        .expect("the crcbl binary runs");
    assert_eq!(code(&output), 1, "a missing file, not a panic");
    assert!(stdout(&output).starts_with(r#"{"ok":false"#));
}

// ---------------------------------------------------------------------------
// `crcbl crpix`
// ---------------------------------------------------------------------------

/// Writes `pixels` as a PNG under `directory` and returns its path.
///
/// `crcbl-golden` is already a dependency of this crate — it is what
/// `screenshot` writes its PNG with — so encoding a fixture costs no new one.
fn write_png(directory: &Path, name: &str, width: u32, height: u32, pixels: Vec<u8>) -> PathBuf {
    let path = directory.join(name);
    crcbl_golden::Image::from_rgba8(width, height, pixels)
        .expect("the fixture's pixels match its size")
        .save_png(&path)
        .expect("the fixture PNG is written");
    path
}

/// The path as the CLI has to be handed it.
fn arg(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 fixture path")
}

/// Four pixels: transparent, red, green, half-alpha blue.
///
/// The transparent one is `0,0,0,0` deliberately. A tracer folds every fully
/// transparent pixel to one colour whatever RGB it carried, so a fixture with
/// leftover colour under its alpha would not compare equal after the round trip
/// for a reason that has nothing to do with this command.
const FOUR: [u8; 16] = [
    0, 0, 0, 0, //
    255, 0, 0, 255, //
    0, 255, 0, 255, //
    0, 0, 255, 128,
];

/// Reads a written sheet back through the format's own parser.
fn parse_sheet(path: &Path) -> crcbl_sprite::crpix::CrpixArt {
    let text = std::fs::read_to_string(path).expect("the sheet was written");
    crcbl_sprite::crpix::parse(&text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

/// The whole point of the command: pixels in, the same pixels out.
///
/// Asserted through `crcbl_sprite::crpix::parse` rather than by eyeballing the
/// text, and on the *pixels* rather than on the file being non-empty — a sheet
/// of the right shape full of the wrong colours would pass any weaker check.
#[test]
fn crpix_round_trips_one_png_to_a_sheet() {
    let temporary = TempDir::new("crpix-one");
    let input = write_png(temporary.path(), "bird-up.png", 2, 2, FOUR.to_vec());
    let output = temporary.path().join("bird.crpix");

    let out = crcbl(
        temporary.path(),
        &["crpix", arg(&input), "-o", arg(&output), "--json"],
    );
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));

    let json = stdout(&out);
    assert_eq!(json.lines().count(), 1, "exactly one line: {json}");
    assert!(
        json.starts_with(r#"{"ok":true,"command":"crpix""#),
        "{json}"
    );
    assert!(has_field(&json, "width", "2"), "{json}");
    assert!(has_field(&json, "height", "2"), "{json}");
    assert!(has_field(&json, "frames", "1"), "{json}");
    assert!(has_field(&json, "colours", "4"), "{json}");
    assert!(has_field(&json, "names", r#"["bird-up"]"#), "{json}");
    assert!(
        has_field(&json, "path", &json_string(&output)),
        "the path a script picks up is in the object: {json}"
    );

    let art = parse_sheet(&output);
    assert_eq!(art.frames.len(), 1);
    // The documented naming: the file stem, not the file name and not an index.
    assert_eq!(art.frames[0].name, "bird-up");
    assert_eq!(art.frames[0].pixels, FOUR, "the pixels changed");
}

/// Frames come out in the order they were typed, which is the only order a
/// person can control. Asserted on frame 2's *pixels*, not on there being
/// three of them: a reversed or sorted list has three frames too.
#[test]
fn crpix_keeps_the_frames_in_command_line_order() {
    let temporary = TempDir::new("crpix-order");
    // Three 2x2 frames, each a different flat colour, so a frame that ends up
    // in the wrong slot is visible in one comparison.
    let flat = |red: u8, green: u8, blue: u8| [red, green, blue, 255].repeat(4);
    let (red, green, blue) = (flat(255, 0, 0), flat(0, 255, 0), flat(0, 0, 255));
    let up = write_png(temporary.path(), "up.png", 2, 2, red.clone());
    let mid = write_png(temporary.path(), "mid.png", 2, 2, green.clone());
    let down = write_png(temporary.path(), "down.png", 2, 2, blue.clone());
    let output = temporary.path().join("bird.crpix");

    // Deliberately not alphabetical order: `down mid up` is what a sort would
    // produce, so typing `up mid down` makes the two answers different.
    let out = crcbl(
        temporary.path(),
        &["crpix", arg(&up), arg(&mid), arg(&down), "-o", arg(&output)],
    );
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));

    let art = parse_sheet(&output);
    assert_eq!(art.frames.len(), 3);
    let names: Vec<&str> = art.frames.iter().map(|frame| frame.name.as_str()).collect();
    assert_eq!(names, ["up", "mid", "down"]);
    assert_eq!(art.frames[0].pixels, red, "frame 1 is the first input");
    assert_eq!(art.frames[1].pixels, green, "frame 2 is the second input");
    assert_eq!(art.frames[2].pixels, blue, "frame 3 is the third input");
}

/// A sheet whose frames are different sizes is not an animation. The refusal
/// names the *file*, which the library cannot do — it only knows frame names.
#[test]
fn crpix_refuses_frames_of_different_sizes_and_names_the_file() {
    let temporary = TempDir::new("crpix-sizes");
    let small = write_png(temporary.path(), "small.png", 2, 2, FOUR.to_vec());
    let tall = write_png(temporary.path(), "tall.png", 2, 3, vec![0u8; 2 * 3 * 4]);
    let output = temporary.path().join("bird.crpix");

    let out = crcbl(
        temporary.path(),
        &[
            "crpix",
            arg(&small),
            arg(&tall),
            "-o",
            arg(&output),
            "--json",
        ],
    );
    assert_eq!(code(&out), 1, "a well-formed request that cannot be met");

    let json = stdout(&out);
    assert!(
        json.starts_with(r#"{"ok":false,"command":"crpix""#),
        "{json}"
    );
    assert!(
        json.contains(&json_fragment(&tall)),
        "the offender is named: {json}"
    );
    assert!(
        json.contains(&json_fragment(&small)),
        "so is what it disagrees with: {json}"
    );
    assert!(json.contains("2x3"), "its own size: {json}");
    assert!(json.contains("2x2"), "and the expected one: {json}");
    assert!(
        !output.exists(),
        "nothing may be written when the inputs are refused"
    );
}

/// Two inputs whose stems collide would be two frames with one name, which a
/// clip could not tell apart. Both paths are named, because the stem they share
/// is the one thing that does not distinguish them.
#[test]
fn crpix_refuses_two_inputs_whose_stems_collide() {
    let temporary = TempDir::new("crpix-dupe");
    let left = temporary.path().join("left");
    let right = temporary.path().join("right");
    std::fs::create_dir_all(&left).expect("a directory");
    std::fs::create_dir_all(&right).expect("a directory");
    let one = write_png(&left, "bird.png", 2, 2, FOUR.to_vec());
    let two = write_png(&right, "bird.png", 2, 2, FOUR.to_vec());
    let output = temporary.path().join("bird.crpix");

    let out = crcbl(
        temporary.path(),
        &["crpix", arg(&one), arg(&two), "-o", arg(&output)],
    );
    assert_eq!(code(&out), 1);
    let message = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(message.contains(arg(&one)), "{message}");
    assert!(message.contains(arg(&two)), "{message}");
    assert!(message.contains("bird"), "{message}");
    assert!(!output.exists());
}

/// A photograph is refused with the library's own reason. What is under test as
/// much as the message is the exit code: 1, not 101, so this is a failure and
/// not a panic escaping through `main`.
#[test]
fn crpix_refuses_art_with_more_colours_than_a_palette_holds() {
    let temporary = TempDir::new("crpix-colours");
    let count = crcbl_sprite::trace::MAX_COLOURS + 88;
    let mut pixels = Vec::with_capacity(count * 4);
    for index in 0..count {
        pixels.extend_from_slice(&[(index % 256) as u8, (index / 256) as u8, 11, 255]);
    }
    let photo = write_png(temporary.path(), "photo.png", count as u32, 1, pixels);
    let output = temporary.path().join("photo.crpix");

    let out = crcbl(
        temporary.path(),
        &["crpix", arg(&photo), "-o", arg(&output), "--json"],
    );
    assert_eq!(code(&out), 1, "a refusal, not a panic");
    let json = stdout(&out);
    assert!(
        json.starts_with(r#"{"ok":false,"command":"crpix""#),
        "{json}"
    );
    assert!(
        json.contains(&crcbl_sprite::trace::MAX_COLOURS.to_string()),
        "the library's own limit is quoted: {json}"
    );
    assert!(
        json.contains("photograph"),
        "and its own explanation: {json}"
    );
    assert!(!output.exists());
}

/// Input that is not readable art fails as a command failure with the CLI's
/// error shape, rather than a panic or a bare exit code.
#[test]
fn crpix_fails_cleanly_on_a_missing_file_and_on_a_file_that_is_not_a_png() {
    let temporary = TempDir::new("crpix-bad");
    let missing = temporary.path().join("nowhere.png");
    let impostor = temporary.path().join("impostor.png");
    std::fs::write(&impostor, b"I am a text file wearing a hat").expect("the impostor");
    // A third input whose *name* holds a backslash, which Unix allows in a file
    // name and JSON has to escape. It is what keeps the escaping half of
    // `json_fragment` honest on a platform whose paths have no separators to
    // escape: without it the helper is the identity function here, and the only
    // machine that ever exercises the escape is Windows CI. On Windows the name
    // is read as two components instead and the case degrades into a second
    // missing file, which the CLI still has to name — so the assertion holds
    // either way and needs no `cfg`.
    let escaped = temporary.path().join(r"back\slash.png");
    let output = temporary.path().join("out.crpix");

    for input in [&missing, &impostor, &escaped] {
        let out = crcbl(
            temporary.path(),
            &["crpix", arg(input), "-o", arg(&output), "--json"],
        );
        assert_eq!(code(&out), 1, "{}", input.display());
        let json = stdout(&out);
        assert_eq!(json.lines().count(), 1, "exactly one line: {json}");
        assert!(
            json.starts_with(r#"{"ok":false,"command":"crpix""#),
            "{json}"
        );
        assert!(json.contains(r#""error":"#), "{json}");
        assert!(
            json.contains(&json_fragment(input)),
            "the file is named: {json}"
        );
        assert!(!output.exists(), "{}", input.display());
    }
}

/// The same "never overwrite unless asked" rule `new` follows, for the same
/// reason: a converter pointed at the wrong `-o` must not eat the file.
#[test]
fn crpix_refuses_to_overwrite_its_output_without_force() {
    let temporary = TempDir::new("crpix-force");
    let input = write_png(temporary.path(), "bird.png", 2, 2, FOUR.to_vec());
    let output = temporary.path().join("bird.crpix");
    std::fs::write(&output, "hand-written, and not to be lost").expect("the occupant");

    let args = ["crpix", arg(&input), "-o", arg(&output)];
    let refused = crcbl(temporary.path(), &args);
    assert_eq!(code(&refused), 1, "the second must not silently overwrite");
    let message = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(message.contains("--force"), "{message}");
    assert_eq!(
        std::fs::read_to_string(&output).expect("still there"),
        "hand-written, and not to be lost",
        "the existing file was modified by a run that said it refused"
    );

    let forced = crcbl(
        temporary.path(),
        &["crpix", arg(&input), "-o", arg(&output), "--force"],
    );
    assert_eq!(
        code(&forced),
        0,
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert_eq!(parse_sheet(&output).frames.len(), 1, "it really overwrote");
}

/// Every option the help text advertises has to change the file, or it is a
/// flag that lies. Read back through the parser, not grepped out of the text.
#[test]
fn crpix_writes_the_nine_slice_sample_mode_and_clip_it_was_given() {
    let temporary = TempDir::new("crpix-options");
    let up = write_png(temporary.path(), "up.png", 4, 4, vec![17u8; 4 * 4 * 4]);
    let down = write_png(temporary.path(), "down.png", 4, 4, vec![34u8; 4 * 4 * 4]);
    let output = temporary.path().join("bird.crpix");

    let out = crcbl(
        temporary.path(),
        &[
            "crpix",
            arg(&up),
            arg(&down),
            "-o",
            arg(&output),
            "--nine",
            "1,2,0,1",
            "--sample",
            "smooth",
            "--clip",
            "flap",
            "--hold",
            "6",
        ],
    );
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));

    let art = parse_sheet(&output);
    assert_eq!(art.nine, Some(crcbl_sprite::NineSlice::new(1, 2, 0, 1)));
    assert_eq!(art.sample, crcbl_sprite::SampleMode::Smooth);
    assert_eq!(art.clips.len(), 1);
    assert_eq!(art.clips[0].name, "flap");
    assert!(art.clips[0].looping);
    // Frame *indices*, resolved by the parser from the names the clip line
    // lists — so this also says the two names it wrote resolve at all.
    assert_eq!(art.clips[0].frames, [0, 1]);
    assert_eq!(art.frames[0].hold, 6, "the clip holds each frame");
    assert_eq!(art.frames[1].hold, 6);
}

/// Insets that overlap leave the centre a negative size, which draws as
/// nothing, and there is no frame to check them against until a PNG has been
/// decoded — so the check cannot live in the parser and has to live here.
///
/// The `.crpix` parser refuses the same thing, which is *why* this asserts on
/// which refusal comes back rather than only on the exit code. Without the
/// command's own check the run still fails, but it fails as "the generated
/// .crpix does not parse, which is a bug in `crcbl crpix`" — telling someone
/// who mistyped a flag that the tool is broken.
#[test]
fn crpix_refuses_a_nine_slice_that_does_not_fit_the_frame() {
    let temporary = TempDir::new("crpix-nine");
    let input = write_png(temporary.path(), "tile.png", 2, 2, FOUR.to_vec());
    let output = temporary.path().join("tile.crpix");

    let out = crcbl(
        temporary.path(),
        &[
            "crpix",
            arg(&input),
            "-o",
            arg(&output),
            "--nine",
            "2,2,0,0",
        ],
    );
    assert_eq!(code(&out), 1);
    let message = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(message.contains("nine-slice"), "{message}");
    assert!(
        message.contains("2x2"),
        "the frame it does not fit: {message}"
    );
    assert!(
        !message.contains("is a bug"),
        "a mistyped flag must not be reported as the tool's own fault: {message}"
    );
    assert!(!output.exists());
}

/// `run` outside a project is a failure with a next step, not a panic and not a
/// silent success.
#[test]
fn run_outside_a_project_fails_with_a_hint() {
    let temporary = TempDir::new("noproject");
    let output = crcbl(temporary.path(), &["run", "--headless", "--json"]);
    // Skipped rather than asserted when the system temp directory happens to
    // sit inside a Cargo project: the test would otherwise be asserting
    // something about the machine rather than about the CLI.
    if code(&output) == 0 {
        return;
    }
    assert_eq!(code(&output), 1);
    let json = stdout(&output);
    assert!(
        json.contains("crcbl new"),
        "the error names a next step: {json}"
    );
}

// ---------------------------------------------------------------------------
// `crcbl lod`
// ---------------------------------------------------------------------------

/// A triangle mesh: positions, and a triangle list over them.
type Mesh = (Vec<[f32; 3]>, Vec<u32>);

/// A `side`×`side` vertex grid over the dune surface, as one triangle list.
///
/// Curved rather than flat because a plane simplifies at no quadric cost at all:
/// every level of its DAG would report an error of zero, and a report that only
/// ever prints zero cannot be told from one that prints the wrong number.
///
/// **The surface is [`crcbl::shaders::dunes::height`]**, which is both the patch
/// the engine ships a committed DAG for and the fixture `crcbl-scene`'s own
/// simplifier tests decimate — so this exercises the report against the geometry
/// the rest of the workspace already reasons about, rather than a third surface.
/// It is also a quartic and calls no `sin`: the results below depend on which
/// edges the decimator collapses, and a transcendental would make them depend on
/// the platform's libm as well.
fn grid(side: usize) -> Mesh {
    assert!(side > 1, "a grid of {side} has no quads");
    let mut positions = Vec::with_capacity(side * side);
    for row in 0..side {
        for column in 0..side {
            let (x, z) = (column as f32, row as f32);
            positions.push([x, crcbl::shaders::dunes::height(x, z), z]);
        }
    }

    let mut indices = Vec::with_capacity((side - 1) * (side - 1) * 6);
    for row in 0..side - 1 {
        for column in 0..side - 1 {
            let corner = (row * side + column) as u32;
            let below = corner + side as u32;
            indices.extend_from_slice(&[corner, below, corner + 1]);
            indices.extend_from_slice(&[corner + 1, below, below + 1]);
        }
    }
    (positions, indices)
}

/// One node of a fixture document, and the mesh it draws.
struct LodNode<'a> {
    /// The node's `name`, which is what the `name_LOD1` convention reads.
    name: &'a str,
    /// The mesh it draws, or `None` for a node that draws nothing.
    mesh: Option<&'a Mesh>,
    /// The body of its `MSFT_lod` extension, written verbatim so a malformed
    /// one can be a fixture too.
    lod_ids: Option<&'a str>,
}

/// Writes `<stem>.gltf` and the `<stem>.bin` it names, and returns the
/// document's path. Every node is in the default scene.
///
/// # Why this is here rather than shared with `crcbl-scene`
///
/// That crate has the same fixture — `gltf_fixture::lod_glb` — and it is
/// `#[cfg(test)] pub(crate)`, so no other crate's tests can see it. The
/// alternatives were checking a `.glb` into the repository, which is a blob
/// nobody reviewing a change could read (the objection `crcbl-scene`'s own
/// fixture module records), or making that module public surface for a test to
/// borrow. This writes glTF's *text* form and a little-endian float dump beside
/// it, so it duplicates no container logic at all: the part `lod_glb` exists for
/// — `.glb` chunk headers and their padding — is exactly the part not repeated
/// here.
fn write_gltf(directory: &Path, stem: &str, nodes: &[LodNode<'_>]) -> PathBuf {
    let mut bin: Vec<u8> = Vec::new();
    let (mut meshes, mut accessors, mut views, mut node_json) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());

    for node in nodes {
        let mesh = node.mesh.map(|(positions, indices)| {
            let position_view = views.len();
            views.push(format!(
                r#"{{"buffer":0,"byteOffset":{},"byteLength":{}}}"#,
                bin.len(),
                positions.len() * 12
            ));
            for position in positions {
                for component in position {
                    bin.extend_from_slice(&component.to_le_bytes());
                }
            }
            views.push(format!(
                r#"{{"buffer":0,"byteOffset":{},"byteLength":{}}}"#,
                bin.len(),
                indices.len() * 4
            ));
            for index in indices {
                bin.extend_from_slice(&index.to_le_bytes());
            }
            accessors.push(format!(
                r#"{{"bufferView":{position_view},"componentType":5126,"count":{},"type":"VEC3"}}"#,
                positions.len()
            ));
            accessors.push(format!(
                r#"{{"bufferView":{},"componentType":5125,"count":{},"type":"SCALAR"}}"#,
                position_view + 1,
                indices.len()
            ));
            meshes.push(format!(
                r#"{{"name":"{}","primitives":[{{"attributes":{{"POSITION":{position_view}}},"indices":{}}}]}}"#,
                node.name,
                position_view + 1
            ));
            meshes.len() - 1
        });

        let mut fields = vec![format!(r#""name":"{}""#, node.name)];
        if let Some(mesh) = mesh {
            fields.push(format!(r#""mesh":{mesh}"#));
        }
        if let Some(ids) = node.lod_ids {
            fields.push(format!(r#""extensions":{{"MSFT_lod":{ids}}}"#));
        }
        node_json.push(format!("{{{}}}", fields.join(",")));
    }

    let scene: Vec<String> = (0..nodes.len()).map(|node| node.to_string()).collect();
    let document = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[{}]}}],"nodes":[{}],
"meshes":[{}],"accessors":[{}],"bufferViews":[{}],
"buffers":[{{"byteLength":{},"uri":"{stem}.bin"}}]}}"#,
        scene.join(","),
        node_json.join(","),
        meshes.join(","),
        accessors.join(","),
        views.join(","),
        bin.len(),
    );

    let path = directory.join(format!("{stem}.gltf"));
    std::fs::write(&path, document).expect("the fixture document is written");
    std::fs::write(directory.join(format!("{stem}.bin")), bin).expect("the fixture buffer");
    path
}

/// A node drawing `mesh`, with no `MSFT_lod`.
fn node<'a>(name: &'a str, mesh: &'a Mesh) -> LodNode<'a> {
    LodNode {
        name,
        mesh: Some(mesh),
        lod_ids: None,
    }
}

/// The whole `--json` object of a `lod` run that has to succeed.
fn lod_json(directory: &Path, args: &[&str]) -> String {
    let mut argv = vec!["lod"];
    argv.extend_from_slice(args);
    argv.push("--json");
    let output = crcbl(directory, &argv);
    assert_eq!(
        code(&output),
        0,
        "{argv:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout(&output)
}

/// A mesh with no hand-authored levels gets a generated chain, and the report
/// carries what the generator measured — not only that levels exist.
///
/// The triangle counts are asserted as a *halving* rather than as literals: the
/// figures belong to the simplifier and would pin this test to its output, but
/// "each level is coarser than the one below and roughly half of it" is the
/// property `docs/plan/25-lod.md` states and the DAG builder targets.
#[test]
fn lod_stats_reports_a_generated_chain_and_the_dag_behind_it() {
    let temporary = TempDir::new("lod-stats");
    let base = grid(24);
    let file = write_gltf(temporary.path(), "plain", &[node("car", &base)]);

    let json = lod_json(temporary.path(), &["stats", arg(&file)]);
    assert_eq!(json.lines().count(), 1, "exactly one line: {json}");
    assert!(json.starts_with(r#"{"ok":true,"command":"lod""#), "{json}");
    assert!(has_field(&json, "action", r#""stats""#), "{json}");
    assert!(
        has_field(&json, "path", &json_string(&file)),
        "the file a script picks up is in the object: {json}"
    );

    // LOD0 is the file's own geometry, and the whole mesh of it.
    assert!(
        json.contains(&format!(
            r#"{{"level":0,"origin":"hand","via":"base","node":0,"node_name":"car","mesh":0,"triangles":{}"#,
            base.1.len() / 3
        )),
        "LOD0 is the base mesh, verbatim: {json}"
    );
    // And every level below it is the generator's, at its own DAG depth.
    for level in 1..4 {
        assert!(
            json.contains(&format!(
                r#"{{"level":{level},"origin":"generated","dag_level":{level},"#
            )),
            "LOD{level} is a uniform cut at depth {level}: {json}"
        );
    }

    let triangles = numbers(&json, "triangles");
    // Chain levels first, then the DAG's own — the same mesh twice, so the
    // sequence restarts. Both halves have to shrink.
    let (chain, dag) = triangles.split_at(triangles.len() / 2);
    assert_eq!(chain, dag, "the chain and its DAG are the same four levels");
    assert!(chain.len() > 2, "a chain of {chain:?} proves nothing");
    for pair in chain.windows(2) {
        assert!(
            pair[1] < pair[0] && pair[1] * 3 > pair[0],
            "levels {chain:?} do not halve",
        );
    }

    // The error curve rises, which is the reason the report exists: a coarser
    // level costs more, and selection reads exactly this number. Split the same
    // way, for the same reason.
    let all = floats(&json, "error_max");
    let (errors, mirror) = all.split_at(all.len() / 2);
    assert_eq!(errors, mirror, "the chain and its DAG report one curve");
    assert_eq!(
        errors[0], 0.0,
        "the base mesh departs from itself by nothing"
    );
    for pair in errors.windows(2) {
        assert!(
            pair[1] > pair[0],
            "the error curve {errors:?} is not rising"
        );
    }

    // Nothing stalled on a mesh that halves cleanly — and the field is there to
    // say so rather than being absent when the answer is "no".
    assert!(has_field(&json, "stalled_levels", "0"), "{json}");
    assert!(has_field(&json, "stalled", "false"), "{json}");
    assert!(!json.contains(r#""stalled":true"#), "{json}");
}

/// The provenance half: a hand-authored level says which node it came out of and
/// what tied it there, and the generator is never run behind it.
#[test]
fn lod_stats_names_the_node_a_hand_authored_level_came_from() {
    let temporary = TempDir::new("lod-hand");
    let (base, coarse, coarser) = (grid(24), grid(12), grid(7));
    let file = write_gltf(
        temporary.path(),
        "hand",
        &[
            node("car", &base),
            node("car_LOD1", &coarse),
            node("car_LOD2", &coarser),
        ],
    );

    let json = lod_json(temporary.path(), &["stats", arg(&file)]);

    assert!(
        json.contains(
            r#"{"level":1,"origin":"hand","via":"name","node":1,"node_name":"car_LOD1","mesh":1"#
        ),
        "LOD1 is node 1's geometry, found by its name: {json}"
    );
    assert!(
        json.contains(
            r#"{"level":2,"origin":"hand","via":"name","node":2,"node_name":"car_LOD2","mesh":2"#
        ),
        "{json}"
    );
    assert!(
        !json.contains(r#""origin":"generated""#),
        "a complete hand-authored chain is never touched by the generator: {json}"
    );
    assert!(
        json.contains(r#""dags":[]"#),
        "and no DAG was built at all: {json}"
    );
    // A hand level below LOD0 was never clustered here, so there is no cluster
    // count and no error to report — and the report says nothing rather than
    // inventing one. LOD0's own clusters would be the only ones, and there is
    // no DAG to have clustered it either.
    assert!(
        !json.contains(r#""clusters""#) && !json.contains(r#""error_max""#),
        "an engine number appeared for geometry the engine never built: {json}"
    );

    // The three nodes are one chain, not three: nodes 1 and 2 are levels of
    // node 0 and are not reported again as bases of their own.
    assert_eq!(
        json.matches(r#"{"node":0,"#).count() + json.matches(r#"{"node":1,"#).count(),
        1,
        "the LOD nodes were reported as chains of their own: {json}"
    );

    // Naming one explicitly still resolves it, because nothing in either
    // convention points upwards.
    let alone = lod_json(temporary.path(), &["stats", arg(&file), "--node", "1"]);
    assert!(
        alone.contains(r#""node":1,"node_name":"car_LOD1""#),
        "{alone}"
    );
}

/// A level that did not halve is said to have not halved, in both renderings.
///
/// This is the finding the report exists to surface rather than to average away:
/// a group whose outer boundary is most of its edges keeps what it has, and the
/// level above it is barely coarser than the level below. It is a real shape —
/// `crates/crcbl-shaders/clusters/dunes.dag` does it at its top levels — and it
/// is invisible in a table of triangle counts nobody reads to the end.
///
/// **The fixture is chosen because it stalls.** A decimator good enough to halve
/// this mesh all the way up would fail the assertion below, and the fix then is
/// a fixture that still stalls, not a check that no longer looks.
#[test]
fn lod_stats_says_which_dag_levels_did_not_halve() {
    let temporary = TempDir::new("lod-stall");
    // Wide enough that the top of the DAG runs out of interior to simplify;
    // a fraction of a second even through the unoptimized binary.
    let base = grid(56);
    let file = write_gltf(temporary.path(), "wide", &[node("car", &base)]);

    let json = lod_json(temporary.path(), &["stats", arg(&file)]);
    let kept = floats(&json, "kept");
    let stalled: Vec<bool> = field_values(&json, "stalled")
        .map(|value| value == "true")
        .collect();

    // Level 0 is below nothing and reports no share, so the flags outnumber the
    // shares by exactly the one level that has none.
    assert_eq!(
        stalled.len(),
        kept.len() + 1,
        "every level is flagged and every level but the first has a share: {json}"
    );
    for (level, &share) in kept.iter().enumerate() {
        assert_eq!(
            stalled[level + 1],
            share > 0.75,
            "level {} kept {share} and was flagged {}: {json}",
            level + 1,
            stalled[level + 1]
        );
    }
    let stalls = stalled.iter().filter(|&&flag| flag).count();
    assert!(stalls > 0, "the fixture stopped stalling: {json}");
    assert!(
        has_field(&json, "stalled_levels", &stalls.to_string()),
        "the summary counts what the levels reported: {json}"
    );

    // And a person reading the table sees it without counting anything.
    let human = stdout(&crcbl(temporary.path(), &["lod", "stats", arg(&file)]));
    assert_eq!(
        human.matches("STALLED").count(),
        stalls,
        "the table hides a stall the JSON reports:\n{human}"
    );
    assert!(
        human.contains(&format!("{stalls} stalled DAG level")),
        "and says so in one line at the top:\n{human}"
    );
}

/// The trap this command exists to avoid: a plausible table for a file that was
/// never read. A refused import is a refusal, and stdout carries no report.
#[test]
fn lod_stats_refuses_a_file_it_could_not_import() {
    let temporary = TempDir::new("lod-broken");
    let broken = temporary.path().join("broken.gltf");
    std::fs::write(&broken, b"this is not a glTF document").expect("the fixture");

    let output = crcbl(temporary.path(), &["lod", "stats", arg(&broken)]);
    assert_eq!(code(&output), 1);
    assert!(
        stdout(&output).is_empty(),
        "a refused import printed a report: {}",
        stdout(&output)
    );
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(message.contains("cannot import"), "{message}");
    assert!(
        message.contains("broken.gltf"),
        "it names the file: {message}"
    );

    // The same failure is machine-readable, and still carries no chain.
    let output = crcbl(temporary.path(), &["lod", "stats", arg(&broken), "--json"]);
    assert_eq!(code(&output), 1);
    let json = stdout(&output);
    assert!(json.starts_with(r#"{"ok":false,"command":"lod""#), "{json}");
    assert!(!json.contains(r#""chains""#), "{json}");

    // And a file that is not there at all is the same kind of answer.
    let output = crcbl(temporary.path(), &["lod", "stats", "absent.gltf"]);
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).is_empty());
}

/// A level two nodes claim is refused by name, rather than ranked. The naming
/// convention and `MSFT_lod` are alternatives, not a precedence order.
#[test]
fn lod_stats_refuses_a_level_two_nodes_claim() {
    let temporary = TempDir::new("lod-conflict");
    let (base, coarse, other) = (grid(24), grid(12), grid(7));
    let file = write_gltf(
        temporary.path(),
        "conflict",
        &[
            LodNode {
                name: "car",
                mesh: Some(&base),
                lod_ids: Some(r#"{"ids":[2]}"#),
            },
            node("car_LOD1", &coarse),
            node("something_else", &other),
        ],
    );

    let output = crcbl(temporary.path(), &["lod", "stats", arg(&file)]);
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(message.contains("LOD1"), "which level: {message}");
    assert!(
        message.contains("node 2") && message.contains("node 1"),
        "both claimants, by name: {message}"
    );
}

/// `gen` writes the cooked artifact, and it is the artifact the renderer's own
/// decoder reads — checked here by decoding it against the very positions the
/// fixture was built from, which is also what proves the levels survived the
/// trip through the importer.
#[test]
fn lod_gen_writes_a_dag_the_decoder_reads_back() {
    let temporary = TempDir::new("lod-gen");
    let base = grid(24);
    let file = write_gltf(temporary.path(), "plain", &[node("car", &base)]);
    let artifact = temporary.path().join("car.dag");

    let json = lod_json(temporary.path(), &["gen", arg(&file), "-o", arg(&artifact)]);
    assert!(has_field(&json, "action", r#""gen""#), "{json}");
    assert!(has_field(&json, "path", &json_string(&artifact)), "{json}");

    let bytes = std::fs::read(&artifact).expect("the artifact was written");
    assert!(
        has_field(&json, "bytes", &bytes.len().to_string()),
        "the reported size is the file's: {json}"
    );

    let dag = crcbl::shaders::cluster_dag::ClusterDag::from_bytes(&bytes, base.0.clone())
        .expect("the artifact decodes against the mesh it was cooked from");
    assert!(
        has_field(&json, "levels", &dag.levels.len().to_string()),
        "the reported level count is the artifact's: {json}"
    );
    assert!(dag.levels.len() > 2, "{} levels", dag.levels.len());
    assert_eq!(
        dag.levels[0].positions, base.0,
        "level 0 is the fixture's own vertices, through the importer unchanged"
    );

    // The per-level figures in the report are the artifact's, not a second
    // count: a report that measured something else would agree on the levels
    // and disagree here.
    for (depth, level) in dag.levels.iter().enumerate() {
        let triangles: u32 = level
            .clusters
            .clusters
            .iter()
            .map(|cluster| cluster.triangle_count)
            .sum();
        assert!(
            json.contains(&format!(
                r#"{{"level":{depth},"triangles":{triangles},"clusters":{},"groups":{}"#,
                level.clusters.clusters.len(),
                level.groups.len()
            )),
            "level {depth} of the artifact is not what was reported: {json}"
        );
    }
}

/// A chain the artist supplied whole has no DAG, and `gen` says so instead of
/// writing an artifact of the base mesh alone.
#[test]
fn lod_gen_refuses_a_fully_hand_authored_chain() {
    let temporary = TempDir::new("lod-gen-hand");
    let (base, coarse) = (grid(24), grid(12));
    let file = write_gltf(
        temporary.path(),
        "hand",
        &[node("car", &base), node("car_LOD1", &coarse)],
    );
    let artifact = temporary.path().join("car.dag");

    let output = crcbl(
        temporary.path(),
        &[
            "lod",
            "gen",
            arg(&file),
            "-o",
            arg(&artifact),
            "--node",
            "0",
        ],
    );
    assert_eq!(code(&output), 1);
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(message.contains("hand-authored"), "{message}");
    assert!(!artifact.exists(), "nothing should have been written");
}

/// The output is not touched when it exists and `--force` was not given — and
/// the check happens before the DAG is built, so a run that refuses cannot have
/// spent a bake first.
#[test]
fn lod_gen_leaves_an_existing_artifact_alone_without_force() {
    let temporary = TempDir::new("lod-gen-force");
    let base = grid(24);
    let file = write_gltf(temporary.path(), "plain", &[node("car", &base)]);
    let artifact = temporary.path().join("taken.dag");
    std::fs::write(&artifact, b"do not tread on me").expect("the file exists");

    let output = crcbl(
        temporary.path(),
        &["lod", "gen", arg(&file), "-o", arg(&artifact)],
    );
    assert_eq!(code(&output), 1);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--force"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(&artifact).expect("still there"),
        b"do not tread on me"
    );

    // And with `--force` it is replaced by a real artifact.
    let output = crcbl(
        temporary.path(),
        &["lod", "gen", arg(&file), "-o", arg(&artifact), "--force"],
    );
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        &std::fs::read(&artifact).expect("rewritten")[..8],
        b"CRCBLDAG"
    );
}

/// `preview` is in `docs/plan/25-lod.md` and is not implemented. It fails saying
/// which of the three is missing, rather than reading as a typo.
#[test]
fn lod_preview_is_refused_as_missing_and_not_as_unknown() {
    let temporary = TempDir::new("lod-preview");
    let output = crcbl(temporary.path(), &["lod", "preview", "any.gltf", "--json"]);
    assert_eq!(
        code(&output),
        1,
        "not implemented is a failed command, not a bad invocation"
    );
    let json = stdout(&output);
    assert!(json.starts_with(r#"{"ok":false,"command":"lod""#), "{json}");
    assert!(has_field(&json, "action", r#""preview""#), "{json}");
    assert!(json.contains("not implemented"), "{json}");
}

/// `crcbl bench` end to end: the distribution, the environment block the plan
/// makes mandatory, and the pool counters that describe the timed calls alone.
///
/// `--workers 0` so the assertion about *which* thread ran the chunks is exact
/// on every runner, single-core CI included; the parallel arm is covered by the
/// subcommand's own tests, where a worker count can be demanded.
#[test]
fn bench_reports_a_distribution_an_environment_and_the_pools_counters() {
    let temporary = TempDir::new("bench");
    // Twenty iterations is exactly the count at which a nearest-rank p95 stops
    // being the maximum, so this is the invocation where all three percentiles
    // must appear. 512 items in chunks of 16 is 32 chunks a call.
    let output = crcbl(
        temporary.path(),
        &[
            "bench",
            "--scenario",
            "jobs",
            "--workers",
            "0",
            "--items",
            "512",
            "--chunk",
            "16",
            "--iterations",
            "20",
            "--warmup",
            "2",
            "--json",
        ],
    );
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout(&output);
    assert_eq!(json.lines().count(), 1, "exactly one line: {json}");
    assert!(
        json.starts_with(r#"{"ok":true,"command":"bench","scenario":"jobs""#),
        "{json}"
    );

    // The environment block: a number without the machine it came from is not
    // comparable to another number.
    assert!(json.contains(r#""environment":{"arch":"#), "{json}");
    for key in ["os", "family", "profile", "parallelism", "workers"] {
        assert!(json.contains(&format!(r#""{key}":"#)), "no {key} in {json}");
    }
    assert!(has_field(&json, "workers", "0"), "{json}");

    // p50 ≤ p95 ≤ p99 ≤ max, and no mean anywhere near them.
    let percentiles: Vec<usize> = ["p50", "p95", "p99", "max"]
        .iter()
        .map(|key| numbers(&json, key)[0])
        .collect();
    assert!(
        percentiles.windows(2).all(|pair| pair[0] <= pair[1]),
        "out of order: {percentiles:?} in {json}"
    );
    assert!(!json.contains("mean"), "{json}");

    // The warm-up ran and was excluded: two calls of 32 chunks warmed up, and
    // the counters that survived the reset cover the twenty timed calls only.
    assert_eq!(numbers(&json, "warmup_chunks"), vec![2 * 32]);
    assert_eq!(numbers(&json, "chunks_run_by_driver"), vec![20 * 32]);
    assert_eq!(numbers(&json, "chunks_run_by_workers"), vec![0]);
}

/// Human output is the default and `--json` is the opt-in, which is the CLI's
/// global rule rather than this subcommand's own — see `bench`'s module docs for
/// why it wins over the plan's "JSON by default".
#[test]
fn bench_prints_a_human_summary_unless_json_is_asked_for() {
    let temporary = TempDir::new("bench-human");
    let output = crcbl(
        temporary.path(),
        &[
            "bench",
            "--scenario",
            "jobs",
            "--workers",
            "0",
            "--items",
            "128",
            "--chunk",
            "16",
            "--iterations",
            "3",
            "--warmup",
            "1",
        ],
    );
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let human = stdout(&output);
    assert!(!human.starts_with('{'), "that is JSON: {human}");
    assert!(human.contains("environment:"), "{human}");
    assert!(human.contains("warm-up:"), "{human}");
    // Three iterations is far below the count a p95 needs, so the run reports
    // its maximum and says so rather than printing one number three times.
    assert!(human.contains("no percentile"), "{human}");
}

/// `crcbl bench --scenario phys` end to end: three distributions, the answer
/// size that explains the third of them, and the tree they all ran against.
///
/// The counts are the point of every assertion here. Nothing below asserts a
/// duration: what is pinned is that each phase produced a full percentile set,
/// that the query phase answered exactly as many results in the timed passes as
/// in the warm-up passes it excluded, and that the tree held the whole crowd.
#[test]
fn bench_phys_reports_three_distributions_and_the_answers_beside_them() {
    let temporary = TempDir::new("bench-phys");
    // Twenty iterations is exactly the count at which a nearest-rank p95 stops
    // being the maximum, so this is the invocation where all three percentiles
    // must appear — for each of the three phases.
    let bodies = 300;
    let iterations = 20;
    let warmup = 2;
    let output = crcbl(
        temporary.path(),
        &[
            "bench",
            "--scenario",
            "phys",
            "--bodies",
            &bodies.to_string(),
            "--extent",
            "16",
            "--iterations",
            &iterations.to_string(),
            "--warmup",
            &warmup.to_string(),
            "--json",
        ],
    );
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout(&output);
    assert_eq!(json.lines().count(), 1, "exactly one line: {json}");
    assert!(
        json.starts_with(r#"{"ok":true,"command":"bench","scenario":"phys""#),
        "{json}"
    );

    // The environment block: a number without the machine it came from is not
    // comparable to another number. No worker count, because this scenario runs
    // on the calling thread and a field describing a pool it never built would
    // be reporting something that was never measured.
    assert!(json.contains(r#""environment":{"arch":"#), "{json}");
    for key in ["os", "family", "profile"] {
        assert!(json.contains(&format!(r#""{key}":"#)), "no {key} in {json}");
    }
    assert!(!json.contains(r#""workers""#), "{json}");

    // Three phases, in build/refit/query order, each a full distribution.
    assert!(
        json.contains(r#""timing":{"build":{"#),
        "the phases are not in order: {json}"
    );
    for key in ["p50", "p95", "p99", "max"] {
        assert_eq!(
            numbers(&json, key).len(),
            3,
            "{key} should appear once per phase: {json}"
        );
    }
    for phase in ["build", "refit", "query"] {
        assert!(json.contains(&format!(r#""{phase}":{{"#)), "{json}");
    }
    // p50 <= p95 <= p99 <= max, within each phase, and no mean beside them.
    for phase in 0..3 {
        let ladder: Vec<usize> = ["p50", "p95", "p99", "max"]
            .iter()
            .map(|key| numbers(&json, key)[phase])
            .collect();
        assert!(
            ladder.windows(2).all(|pair| pair[0] <= pair[1]),
            "phase {phase} is out of order: {ladder:?} in {json}"
        );
    }
    assert!(!json.contains("mean"), "{json}");

    // The answer size, which is what makes the query timing readable. One query
    // per body, and more results than queries — a crowd where every query found
    // only itself would be a fixture with no neighbourhood in it.
    assert_eq!(numbers(&json, "queries"), vec![bodies]);
    let results = numbers(&json, "results")[0];
    assert!(results > bodies, "{results} results over {bodies} queries");
    assert!(floats(&json, "results_per_query")[0] > 1.0, "{json}");

    // The tree held the whole crowd, so the queries answered for the crowd that
    // was placed rather than for part of it.
    assert_eq!(numbers(&json, "elements"), vec![bodies]);

    // The warm-up ran and was excluded: every pass answers the same results, so
    // both totals are that number times the passes each half ran.
    assert_eq!(numbers(&json, "warmup_results"), vec![warmup * results]);
    assert_eq!(numbers(&json, "timed_results"), vec![iterations * results]);

    // A full 64-bit fold, carried as a *string* so a consumer reading JSON
    // numbers as doubles cannot round the low bits off the one field whose whole
    // job is to compare exactly.
    let checksum = field_values(&json, "checksum").next().expect("a checksum");
    assert!(
        checksum.starts_with('"') && checksum.ends_with('"'),
        "the checksum is a JSON number and will be rounded: {checksum}"
    );
    assert!(
        checksum.trim_matches('"').parse::<u64>().is_ok(),
        "{checksum}"
    );
}

/// **The same crowd in a smaller arena answers more per query**, which is the
/// fact `docs/backlog.md` says a scale number is meaningless without.
///
/// Two runs at one body count, and the assertions are on the *answers*: a
/// timing assertion is not a test, and the reason this scenario reports its
/// answer size is precisely so the two query numbers can be read against
/// something that is not the body count.
#[test]
fn bench_phys_answers_more_per_query_in_a_denser_arena() {
    let temporary = TempDir::new("bench-phys-density");
    let bodies = "300";
    let run = |extent: &str| {
        let output = crcbl(
            temporary.path(),
            &[
                "bench",
                "--scenario",
                "phys",
                "--bodies",
                bodies,
                "--extent",
                extent,
                "--iterations",
                "3",
                "--warmup",
                "1",
                "--json",
            ],
        );
        assert_eq!(
            code(&output),
            0,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        stdout(&output)
    };

    let dense = run("12");
    let sparse = run("96");
    let neighbours = |json: &str| floats(json, "results_per_query")[0];
    assert!(
        neighbours(&dense) > neighbours(&sparse),
        "{} per query at extent 12 is not more than {} at extent 96",
        neighbours(&dense),
        neighbours(&sparse)
    );
    // Both are real passes: every body answers its own query at any density.
    assert!(neighbours(&sparse) >= 1.0, "{sparse}");
    // The same crowd either way — the extent moves where the bodies are, never
    // how many there are.
    assert_eq!(numbers(&dense, "elements"), numbers(&sparse, "elements"));

    // And two densities that folded one checksum would not be two densities.
    let checksum = |json: &str| {
        field_values(json, "checksum")
            .next()
            .expect("a checksum")
            .to_string()
    };
    assert_ne!(checksum(&dense), checksum(&sparse));
}

/// **`--ticks` ages the crowd, and the tree it is queried through keeps the
/// shape the build gave it** — `docs/backlog.md`'s finding, end to end.
///
/// Two runs of one invocation apart from the tick count. What the flag changes
/// is the crowd: the aged run folds a different checksum and reports one refit
/// per body *per tick*, which is a count no run that parsed the flag and then
/// ignored it can reach. What it does not change is the tree: `nodes` and
/// `depth` are identical, because a refit rewrites boxes and never topology,
/// and `rebuilds` stays at the build phase's one — the crowd walked away and
/// nothing re-fitted it to where it went.
///
/// Nothing here asserts a duration. Whether the looser tree is *slower* is what
/// a sweep answers, and a timing assertion is not a test.
#[test]
fn bench_phys_ticks_age_the_crowd_without_reshaping_the_tree() {
    let temporary = TempDir::new("bench-phys-ticks");
    let bodies = 300;
    let run = |ticks: &str| {
        let output = crcbl(
            temporary.path(),
            &[
                "bench",
                "--scenario",
                "phys",
                "--bodies",
                &bodies.to_string(),
                "--extent",
                "16",
                "--ticks",
                ticks,
                "--iterations",
                "3",
                "--warmup",
                "1",
                "--json",
            ],
        );
        assert_eq!(
            code(&output),
            0,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        stdout(&output)
    };

    let ticks = 64;
    let fresh = run("1");
    let aged = run(&ticks.to_string());

    // The run says what it was asked for, so a reader of the JSON alone can
    // tell the two apart.
    assert_eq!(numbers(&fresh, "ticks"), vec![1]);
    assert_eq!(numbers(&aged, "ticks"), vec![ticks]);

    // Every tick refits every body: the observable that says the ageing steps
    // ran rather than being parsed and dropped.
    assert_eq!(numbers(&fresh, "refits"), vec![bodies]);
    assert_eq!(numbers(&aged, "refits"), vec![bodies * ticks]);

    // And the crowd really is somewhere else, which the checksum is the only
    // field that can say.
    let checksum = |json: &str| {
        field_values(json, "checksum")
            .next()
            .expect("a checksum")
            .to_string()
    };
    assert_ne!(checksum(&fresh), checksum(&aged));

    // The tree kept the shape `Bvh::build` gave it for where the crowd
    // started: same nodes, same depth, and no second build.
    for key in ["elements", "nodes", "depth"] {
        assert_eq!(
            numbers(&fresh, key),
            numbers(&aged, key),
            "{key} moved, so something re-picked a leaf's place"
        );
    }
    assert_eq!(numbers(&aged, "rebuilds"), vec![1]);
    assert_eq!(numbers(&aged, "updates_without_refit"), vec![0]);
}

/// An unknown scenario is a bad invocation, and it names the ones that exist —
/// exit 2 with nothing on stdout, like every other malformed invocation.
#[test]
fn bench_refuses_an_unknown_scenario_by_name() {
    let temporary = TempDir::new("bench-scenario");
    let output = crcbl(temporary.path(), &["bench", "--scenario", "frobnicate"]);
    assert_eq!(code(&output), 2);
    assert!(output.stdout.is_empty(), "{}", stdout(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("frobnicate"), "{stderr}");
    for scenario in ["jobs", "phys"] {
        assert!(stderr.contains(scenario), "{stderr}");
    }
}

// ---------------------------------------------------------------------------
// sim — the determinism harness
// ---------------------------------------------------------------------------
//
// These were the `crcbl-sim` binary's own headless tests until the harness
// moved behind the `crcbl sim` verb, and they are the same tests against the
// same contract: the arguments, the exit codes and the one-line output. They
// are here rather than in the subcommand's own `#[cfg(test)]` module for the
// reason this file exists at all — the compiled binary is what CI and a
// developer actually invoke, and only a separate target can spawn it.
//
// The determinism harness had no tests at all before that file was written,
// which is how `--tick-rate 0` survived: it parsed cleanly and then divided by
// zero computing the tick period, aborting with exit 101 rather than the
// documented exit 2. `a_malformed_invocation_exits_two` above is where that
// case now lives, alongside every other verb's.

/// `hash:<hex> ticks:<n> final_tick:<n>` — the whole output contract.
///
/// Panics rather than returning an `Option` for each field, so a run that
/// printed something else fails naming what it printed instead of making every
/// assertion downstream vacuous.
fn sim_parts(line: &str) -> (String, u64, u64) {
    let mut hash = None;
    let mut ticks = None;
    let mut final_tick = None;
    for field in line.split_whitespace() {
        match field.split_once(':') {
            Some(("hash", v)) => hash = Some(v.to_owned()),
            Some(("ticks", v)) => ticks = v.parse().ok(),
            Some(("final_tick", v)) => final_tick = v.parse().ok(),
            _ => {}
        }
    }
    (
        hash.unwrap_or_else(|| panic!("no hash in {line:?}")),
        ticks.unwrap_or_else(|| panic!("no ticks in {line:?}")),
        final_tick.unwrap_or_else(|| panic!("no final_tick in {line:?}")),
    )
}

#[test]
fn sim_default_run_exits_zero_and_prints_its_contract() {
    let temporary = TempDir::new("sim");
    let output = crcbl(temporary.path(), &["sim"]);
    assert_eq!(
        code(&output),
        0,
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (hash, ticks, final_tick) = sim_parts(&stdout(&output));
    assert_eq!(hash.len(), 16, "hash is 16 hex digits: {hash}");
    assert_eq!(ticks, 1_000);
    assert_eq!(final_tick, 1_000);
}

/// **The two numbers must agree.** The loop used to break out of the tick drain
/// the moment the budget was met, leaving whole ticks in the accumulator that
/// the clock had already counted — so a determinism harness could report a tick
/// count and a final tick that differed for a reason unrelated to determinism.
#[test]
fn sim_tick_count_and_final_tick_always_agree() {
    let temporary = TempDir::new("sim-agree");
    for n in ["1", "7", "60", "997"] {
        let output = crcbl(temporary.path(), &["sim", "--ticks", n]);
        assert_eq!(code(&output), 0);
        let (_, ticks, final_tick) = sim_parts(&stdout(&output));
        assert_eq!(ticks, n.parse::<u64>().unwrap(), "--ticks {n}");
        assert_eq!(final_tick, ticks, "--ticks {n}");
    }
}

/// Same input, same hash — the property the harness exists to assert.
#[test]
fn sim_same_input_produces_the_same_hash() {
    let temporary = TempDir::new("sim-determinism");
    let run = |args: &[&str]| stdout(&crcbl(temporary.path(), args));

    let first = run(&["sim", "--ticks", "500", "--seed", "42"]);
    let second = run(&["sim", "--ticks", "500", "--seed", "42"]);
    assert_eq!(first, second);

    let other_seed = run(&["sim", "--ticks", "500", "--seed", "43"]);
    assert_ne!(
        sim_parts(&first).0,
        sim_parts(&other_seed).0,
        "a different seed must build a different world",
    );

    let other_length = run(&["sim", "--ticks", "501", "--seed", "42"]);
    assert_ne!(
        sim_parts(&first).0,
        sim_parts(&other_length).0,
        "the hash must depend on how long the world ran",
    );
}

/// The tick rate changes the clock, not the number of ticks: the harness
/// advances by exactly one period per iteration at any rate.
#[test]
fn sim_tick_rate_does_not_change_the_tick_count() {
    let temporary = TempDir::new("sim-rate");
    for rate in ["1", "30", "60", "240"] {
        let output = crcbl(
            temporary.path(),
            &["sim", "--ticks", "100", "--tick-rate", rate],
        );
        assert_eq!(code(&output), 0, "--tick-rate {rate}");
        let (_, ticks, final_tick) = sim_parts(&stdout(&output));
        assert_eq!((ticks, final_tick), (100, 100), "--tick-rate {rate}");
    }
}

/// `sim --help` documents the contract a script parses, not just its flags.
///
/// The per-command help sweep in `help_and_version_exit_zero` already proves
/// every verb's `--help` exits 0; this is the part specific to `sim`, and it
/// includes what the verb deliberately does *not* take, so a reader is not left
/// looking for the scene argument topic 11 sketches.
#[test]
fn sim_help_documents_the_contract_and_what_it_does_not_take() {
    let temporary = TempDir::new("sim-help");
    let help = crcbl(temporary.path(), &["sim", "--help"]);
    assert_eq!(code(&help), 0);
    let text = stdout(&help);
    for needle in [
        "--ticks",
        "--tick-rate",
        "--seed",
        "hash:<hex> ticks:<n> final_tick:<n>",
        "script.ron",
        "--hash",
    ] {
        assert!(text.contains(needle), "no `{needle}` in:\n{text}");
    }
}

/// A zero-tick run is a legal degenerate case, not a crash.
#[test]
fn sim_zero_ticks_is_a_run_of_length_zero() {
    let temporary = TempDir::new("sim-zero");
    let output = crcbl(temporary.path(), &["sim", "--ticks", "0"]);
    assert_eq!(code(&output), 0);
    let (_, ticks, final_tick) = sim_parts(&stdout(&output));
    assert_eq!((ticks, final_tick), (0, 0));
}

/// `--json` is the design rule topic 11 states for every subcommand, and what
/// it emits is the human line's three fields under stable keys — the hash
/// spelled identically, so a consumer holding both can compare them.
#[test]
fn sim_json_carries_the_same_hash_the_human_line_prints() {
    let temporary = TempDir::new("sim-json");
    let args = ["sim", "--ticks", "64", "--seed", "9", "--tick-rate", "120"];
    let human = stdout(&crcbl(temporary.path(), &args));
    let (hash, ticks, final_tick) = sim_parts(&human);

    let mut json_args = args.to_vec();
    json_args.push("--json");
    let output = crcbl(temporary.path(), &json_args);
    assert_eq!(code(&output), 0);
    let json = stdout(&output);
    assert_eq!(json.lines().count(), 1, "exactly one line: {json}");
    assert!(json.starts_with(r#"{"ok":true,"command":"sim""#), "{json}");
    assert!(has_field(&json, "hash", &format!("\"{hash}\"")), "{json}");
    assert!(has_field(&json, "ticks", &ticks.to_string()), "{json}");
    assert!(
        has_field(&json, "final_tick", &final_tick.to_string()),
        "{json}"
    );
    // The inputs come back with the answer, so one `--json` line is a complete
    // record of the run rather than something to be read beside its command.
    assert!(has_field(&json, "seed", r#""9""#), "{json}");
    assert!(has_field(&json, "tick_rate", "120"), "{json}");
}

/// Every `"<key>":<integer>` in the object, in order.
fn numbers(json: &str, key: &str) -> Vec<usize> {
    field_values(json, key)
        .map(|value| value.parse().unwrap_or_else(|_| panic!("{key}: {value}")))
        .collect()
}

/// Every `"<key>":<number>` in the object as a float, in order.
fn floats(json: &str, key: &str) -> Vec<f32> {
    field_values(json, key)
        .map(|value| value.parse().unwrap_or_else(|_| panic!("{key}: {value}")))
        .collect()
}

/// The raw text of every `"<key>":<value>` up to the next `,` or `}`.
///
/// Asserts it found at least one: a probe that silently matched nothing would
/// make every assertion over its result vacuously true.
fn field_values<'a>(json: &'a str, key: &str) -> impl Iterator<Item = &'a str> {
    let needle = format!("\"{key}\":");
    let values: Vec<&str> = json
        .match_indices(&needle)
        .map(|(at, _)| {
            let rest = &json[at + needle.len()..];
            let end = rest
                .find([',', '}'])
                .unwrap_or_else(|| panic!("{key} at {at} is not terminated: {rest}"));
            &rest[..end]
        })
        .collect();
    assert!(!values.is_empty(), "no `{key}` in {json}");
    values.into_iter()
}

// ---------------------------------------------------------------------------
// settings — a game's settings.toml, from a terminal
// ---------------------------------------------------------------------------
//
// Every one of these points `--config-dir` at a directory of the test's own, so
// nothing here can read or write the developer's real `~/.config`. That flag is
// why they can: `dirs` resolves the config directory from `XDG_CONFIG_HOME` on
// Linux and from a Windows known folder on Windows, so there is no environment
// variable that redirects it everywhere, and a suite that redirected it on the
// two platforms where it works would be silently writing to a real machine on
// the third.

use crcbl_store::NativeStorage;
use crcbl_store::settings::{SETTINGS_FILE, SettingsStack};

/// Runs `crcbl settings` with its config root inside `home`.
///
/// The working directory is `home` too, which has no project above it that this
/// suite put there — every one of these passes `--app`, so nothing depends on
/// what a manifest search would find.
fn settings(home: &Path, args: &[&str]) -> Output {
    let mut argv: Vec<&str> = vec!["settings", "--config-dir", arg(home), "--app", "testgame"];
    argv.extend_from_slice(args);
    crcbl(home, &argv)
}

/// The directory `--config-dir <home> --app testgame` resolves to.
fn settings_root(home: &Path) -> PathBuf {
    home.join("testgame")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// **`set` then `get` gives the value back**, for each kind the verb writes.
#[test]
fn a_value_written_by_set_is_the_value_get_reports() {
    let temporary = TempDir::new("settings-roundtrip");
    let home = temporary.path();

    for (key, typed, expected, kind) in [
        ("engine.video.shadows", "false", "false", "boolean"),
        ("engine.audio.channels", "2", "2", "integer"),
        ("engine.audio.master_volume", "0.8", "0.8", "float"),
        ("game.difficulty", "normal", "normal", "string"),
        // The escape hatch: quoted, so it is the four characters and not the
        // boolean. The shell layer is gone here — this is the argv the shell
        // would have produced.
        ("game.label", "\"true\"", "true", "string"),
    ] {
        let written = settings(home, &["set", key, typed]);
        assert_eq!(code(&written), 0, "set {key}: {}", stderr(&written));
        assert!(
            stdout(&written).contains(&format!("({kind})")),
            "set {key} did not report it as a {kind}: {}",
            stdout(&written)
        );

        let read = settings(home, &["get", key]);
        assert_eq!(code(&read), 0, "get {key}: {}", stderr(&read));
        assert_eq!(stdout(&read).trim_end(), expected, "get {key}");

        let json = stdout(&settings(home, &["--json", "get", key]));
        assert!(has_field(&json, "present", "true"), "{json}");
        assert!(has_field(&json, "type", &format!("\"{kind}\"")), "{json}");
    }
}

/// **A boolean this verb writes is a boolean in the file**, read back by
/// `SettingsStack` itself rather than by the verb that wrote it.
///
/// `crcbl-store` skips a value it cannot deserialize, so `shadows = "false"`
/// and no `shadows` key at all are the same answer to `get::<bool>`. A `set`
/// that wrote the string would therefore produce a file that looks right in
/// every other test here — `settings get` would even read it back — and does
/// nothing at all when a game starts.
#[test]
fn a_boolean_written_by_set_is_a_boolean_to_the_store_that_reads_it() {
    let temporary = TempDir::new("settings-typed");
    let home = temporary.path();

    assert_eq!(
        code(&settings(home, &["set", "engine.video.shadows", "false"])),
        0
    );
    assert_eq!(
        code(&settings(home, &["set", "game.label", "\"false\""])),
        0
    );

    let storage = NativeStorage::at(settings_root(home));
    let stack = SettingsStack::from_storage(&storage);
    assert_eq!(
        stack.get::<bool>("engine.video.shadows"),
        Some(false),
        "the engine reads this key as a bool and got nothing"
    );
    assert_eq!(
        stack.get::<bool>("game.label"),
        None,
        "a quoted value must stay text"
    );
    assert_eq!(stack.get::<String>("game.label").as_deref(), Some("false"));
}

/// **A setting this verb writes is one the engine reads.**
///
/// The end of the chain the other tests only cover halves of:
/// `crcbl::settings::video_effects` is what `GpuContext::open` calls at
/// start-up, and it is the consumer that decides whether writing
/// `engine.video.shadows = false` from a terminal actually switches the shadow
/// pass off. Without this the verb could write a file every test here agrees
/// is correct and the engine ignores.
#[test]
fn a_setting_written_from_the_cli_reaches_the_engine_that_reads_it() {
    use crcbl::render::RenderEffects;
    use crcbl::settings::video_effects;

    let temporary = TempDir::new("settings-engine");
    let home = temporary.path();
    let storage = NativeStorage::at(settings_root(home));

    // Nothing written yet: the player has asked for nothing, so nothing is
    // clamped. The arm that fails if an absent key ever reads as `false`.
    assert_eq!(
        video_effects(&SettingsStack::from_storage(&storage)),
        RenderEffects::all()
    );

    assert_eq!(
        code(&settings(home, &["set", "engine.video.shadows", "false"])),
        0
    );

    assert_eq!(
        video_effects(&SettingsStack::from_storage(&storage)),
        RenderEffects::all().difference(RenderEffects::SHADOWS),
        "`crcbl settings set` wrote a file the engine does not read"
    );
}

/// **`list` shows what was set**, in both renderings, and says which file it
/// read.
#[test]
fn list_shows_the_file_it_read_and_everything_in_it() {
    let temporary = TempDir::new("settings-list");
    let home = temporary.path();
    let path = settings_root(home).join(SETTINGS_FILE);

    // Before anything exists, `list` still answers and names the file.
    let empty = settings(home, &["list"]);
    assert_eq!(code(&empty), 0, "{}", stderr(&empty));
    assert!(stdout(&empty).contains("no file yet"), "{}", stdout(&empty));
    assert!(stdout(&empty).contains(arg(&path)), "{}", stdout(&empty));

    assert_eq!(
        code(&settings(home, &["set", "engine.video.shadows", "false"])),
        0
    );
    assert_eq!(
        code(&settings(home, &["set", "game.difficulty", "hard"])),
        0
    );

    let listed = settings(home, &["list"]);
    assert_eq!(code(&listed), 0, "{}", stderr(&listed));
    let human = stdout(&listed);
    assert!(human.contains(arg(&path)), "{human}");
    assert!(human.contains("shadows = false"), "{human}");
    assert!(human.contains("difficulty = \"hard\""), "{human}");

    let json = stdout(&settings(home, &["--json", "list"]));
    assert!(has_field(&json, "file_exists", "true"), "{json}");
    assert!(has_field(&json, "count", "2"), "{json}");
    assert!(
        json.contains(r#"{"key":"engine.video.shadows","type":"boolean","value":false}"#),
        "{json}"
    );
    assert!(
        json.contains(r#"{"key":"game.difficulty","type":"string","value":"hard"}"#),
        "{json}"
    );
    assert!(json.contains(&json_string(&path)), "{json}");
}

/// **A key that is not set is an answer, not a failure**, and it is not
/// mistakable for a value: stdout carries nothing a script could read as one.
#[test]
fn get_of_a_key_that_is_not_set_answers_and_exits_zero() {
    let temporary = TempDir::new("settings-absent");
    let home = temporary.path();

    let absent = settings(home, &["get", "engine.video.shadows"]);
    assert_eq!(
        code(&absent),
        0,
        "an unset key is not a failure of the tool"
    );
    assert!(
        stdout(&absent).trim().is_empty(),
        "stdout must hold nothing a script would read as the value: {:?}",
        stdout(&absent)
    );
    assert!(
        stderr(&absent).contains("is not set"),
        "and a person has to be told why it was empty: {:?}",
        stderr(&absent)
    );

    let json = stdout(&settings(home, &["--json", "get", "engine.video.shadows"]));
    assert!(has_field(&json, "ok", "true"), "{json}");
    assert!(has_field(&json, "present", "false"), "{json}");
    assert!(
        !json.contains("\"value\":"),
        "an absent key has no value field to misread: {json}"
    );

    // A key holding something `get` does not render is the *other* answer, and
    // `SettingsStack::contains` is the only thing that tells the two apart.
    assert_eq!(
        code(&settings(home, &["set", "engine.video.shadows", "false"])),
        0
    );
    let table = settings(home, &["get", "engine.video"]);
    assert_eq!(
        code(&table),
        1,
        "a table is a failed `get`, not an absent key"
    );
    assert!(stderr(&table).contains("list"), "{}", stderr(&table));
}

/// **Reading creates nothing; only `set` creates the config directory.**
///
/// `SettingsStack::platform` is deliberately `mkdir`-free so that a start-up on
/// a machine that has never had a settings file leaves it that way, and a CLI
/// that created the directory just to report an empty file would undo that for
/// every developer who ran `crcbl settings list` once.
#[test]
fn only_set_creates_the_config_directory() {
    let temporary = TempDir::new("settings-mkdir");
    let home = temporary.path();
    let root = settings_root(home);

    assert_eq!(code(&settings(home, &["list"])), 0);
    assert_eq!(code(&settings(home, &["get", "engine.video.shadows"])), 0);
    assert!(
        !root.exists(),
        "reading settings created {}",
        root.display()
    );

    assert_eq!(
        code(&settings(home, &["set", "engine.video.shadows", "false"])),
        0
    );
    assert!(
        root.join(SETTINGS_FILE).is_file(),
        "set wrote nothing to {}",
        root.display()
    );
}

/// **Without `--app` the game is the project in the current directory** — the
/// same project `crcbl run` and `crcbl build` act on, found by the same search.
#[test]
fn the_game_defaults_to_the_project_in_the_current_directory() {
    let temporary = TempDir::new("settings-project");
    let project = temporary.path().join("settingsgame");
    let inside = project.join("src");
    std::fs::create_dir_all(&inside).expect("a project to stand in");
    std::fs::write(
        project.join("Cargo.toml"),
        b"[package]\nname = \"settingsgame\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("a manifest");
    let home = temporary.path().join("config");

    // From a subdirectory, because the manifest search goes upwards.
    let output = crcbl(
        &inside,
        &[
            "settings",
            "--config-dir",
            arg(&home),
            "--json",
            "set",
            "game.difficulty",
            "hard",
        ],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let json = stdout(&output);
    assert!(has_field(&json, "app", "\"settingsgame\""), "{json}");
    assert!(
        home.join("settingsgame").join(SETTINGS_FILE).is_file(),
        "the file did not land under the package's own name"
    );
}

/// **A directory with no game in it is refused, not guessed at.**
///
/// A virtual workspace root has a `Cargo.toml` and no package, so the search
/// succeeds and there is still no name — the case where inventing one would put
/// a player's settings somewhere nothing will ever read them.
#[test]
fn a_manifest_with_no_package_refuses_and_names_the_flag_that_fixes_it() {
    let temporary = TempDir::new("settings-noproject");
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("a workspace root");
    std::fs::write(
        workspace.join("Cargo.toml"),
        b"[workspace]\nmembers = []\nresolver = \"2\"\n",
    )
    .expect("a manifest");
    let home = temporary.path().join("config");

    let output = crcbl(
        &workspace,
        &["settings", "--config-dir", arg(&home), "list"],
    );
    assert_eq!(code(&output), 1, "{}", stdout(&output));
    assert!(stderr(&output).contains("--app"), "{}", stderr(&output));
    assert!(!home.exists(), "a refusal must create nothing");
}

/// **A settings file that will not parse fails the command, and `set` does not
/// write over it.**
///
/// `SettingsStack::platform` turns an unreadable file into an empty layer and a
/// log line, because a game's start-up must not die over one — and that is the
/// wrong answer here: a `set` on top of it would serialise the empty layer back
/// and the player's file would be gone. So this verb loads the file itself and
/// a parse error is a failure.
#[test]
fn a_settings_file_that_is_not_toml_fails_the_command_and_is_left_alone() {
    let temporary = TempDir::new("settings-broken");
    let home = temporary.path();
    let root = settings_root(home);
    std::fs::create_dir_all(&root).expect("a config directory");
    let path = root.join(SETTINGS_FILE);
    let broken = b"this is not = = toml\n";
    std::fs::write(&path, broken).expect("a broken settings file");

    for args in [
        vec!["list"],
        vec!["get", "engine.video.shadows"],
        vec!["set", "engine.video.shadows", "false"],
    ] {
        let output = settings(home, &args);
        assert_eq!(code(&output), 1, "{args:?}: {}", stdout(&output));
        assert!(
            stderr(&output).contains("cannot read"),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    assert_eq!(
        std::fs::read(&path).expect("the file is still there"),
        broken,
        "a refused command rewrote the player's file"
    );
}

/// **An ancestor holding a scalar is reported, not clobbered.**
///
/// A hand-edited `settings.toml` can put anything anywhere, and `engine = "x"`
/// makes `engine.video.shadows` a key with nowhere to go. `crcbl-store` refuses
/// that write; what is asserted here is that the refusal reaches the exit code
/// and that whatever else the file held survives.
#[test]
fn a_set_under_a_scalar_ancestor_fails_rather_than_discarding_it() {
    let temporary = TempDir::new("settings-ancestor");
    let home = temporary.path();
    let root = settings_root(home);
    std::fs::create_dir_all(&root).expect("a config directory");
    let path = root.join(SETTINGS_FILE);
    let original = b"engine = \"not a table\"\n";
    std::fs::write(&path, original).expect("a hand-edited settings file");

    let output = settings(home, &["set", "engine.video.shadows", "false"]);
    assert_eq!(code(&output), 1, "{}", stdout(&output));
    assert_eq!(
        std::fs::read(&path).expect("the file is still there"),
        original,
        "a refused set rewrote the player's file"
    );
}
