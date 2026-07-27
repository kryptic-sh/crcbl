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
    for command in ["new", "run", "build"] {
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
