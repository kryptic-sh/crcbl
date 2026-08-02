//! End-to-end tests of the `crcbl` binary that do not compile anything.
//!
//! These run in the ordinary suite on every platform: they invoke the real
//! binary, assert the exit-code contract and the `--json` shapes, and scaffold
//! into a temporary directory. What they deliberately do *not* do is build the
//! scaffolded project — that takes a full engine compile, so it lives in
//! `cli_e2e.rs` behind the `cli-e2e` feature and its own CI job, exactly as the
//! shell crate's two display-dependent suites do.
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
    for command in ["new", "run", "build", "screenshot", "replay", "crpix"] {
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
        has_field(
            &json,
            "path",
            &format!("{:?}", target.to_str().expect("a UTF-8 path"))
        ),
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
        has_field(&json, "path", &format!("{:?}", arg(&output))),
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
    assert!(json.contains(arg(&tall)), "the offender is named: {json}");
    assert!(
        json.contains(arg(&small)),
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
    let output = temporary.path().join("out.crpix");

    for input in [&missing, &impostor] {
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
        assert!(json.contains(arg(input)), "the file is named: {json}");
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
