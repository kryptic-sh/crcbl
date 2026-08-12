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

/// wasm is P5. The refusal is machine-readable, so a CI job can tell "not yet"
/// from "your flag is wrong" without matching prose.
#[test]
fn building_for_wasm_fails_cleanly_with_a_phase() {
    let temporary = TempDir::new("wasm");
    let output = crcbl(temporary.path(), &["build", "--target", "wasm", "--json"]);
    assert_eq!(code(&output), 1, "a well-formed request that cannot be met");
    let json = stdout(&output);
    assert!(
        json.starts_with(r#"{"ok":false,"command":"build""#),
        "{json}"
    );
    assert!(has_field(&json, "phase", r#""P5""#), "{json}");
    assert!(json.contains("P5"), "the human message names the phase too");

    // Without `--json` the same refusal goes to stderr and stdout stays clean,
    // so a shell pipeline is never fed an error message as data.
    let output = crcbl(temporary.path(), &["build", "--target", "wasm"]);
    assert_eq!(code(&output), 1);
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("P5"));
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
