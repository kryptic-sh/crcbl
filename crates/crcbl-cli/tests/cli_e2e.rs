//! The scripted session the CLI exists for: `new` → it compiles → it runs
//! headless → `build`.
//!
//! **Compiled only with the `cli-e2e` feature**, and `#[ignore]`d on top of
//! that, which nothing but `tests/run-cli-e2e.sh` and the CI job it drives
//! turns on. Same terms as `crcbl-shell`'s two display-dependent suites and for
//! a related reason: this test compiles a whole engine into a fresh target
//! directory, which is a minute of CI rather than a millisecond, and
//! `docs/plan/12-testing.md` wants a plain `cargo nextest run` to stay fast.
//! The harness fails when the suite reports zero tests run, so gating it cannot
//! quietly turn into skipping it.
//!
//! What it pins down that `cli.rs` cannot: that the *template compiles*. A
//! scaffold that does not build is worse than no scaffold — it is a broken
//! first impression that no unit test of the generator would ever catch,
//! because the generator's job is producing text and the text's job is being
//! valid Rust.

#![cfg(feature = "cli-e2e")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn engine_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the CLI lives two levels below the workspace root")
        .to_path_buf()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("crcbl-cli-{label}-{}", std::process::id()));
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

/// Runs a command and returns its output, failing loudly with both streams.
fn run(what: &str, command: &mut Command) -> Output {
    let output = command.output().unwrap_or_else(|error| {
        panic!("{what}: could not start: {error}");
    });
    assert!(
        output.status.success(),
        "{what}: exit {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

/// A `cargo` invocation for the *scaffolded* project.
///
/// `CARGO_TARGET_DIR` is set explicitly rather than inherited: a CI job that
/// exports one globally would otherwise point this nested build at the outer
/// build's target directory, where it would block on the same lock the test
/// runner is already holding. That is a deadlock, not a slowdown.
fn scaffold_cargo(project: &Path, target: &Path) -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(project);
    command.env("CARGO_TARGET_DIR", target);
    command
}

#[test]
#[ignore = "compiles a whole engine; run through tests/run-cli-e2e.sh"]
fn a_scaffolded_project_builds_lints_and_runs_headless() {
    let temporary = TempDir::new("e2e");
    let engine = engine_root();
    let crcbl = env!("CARGO_BIN_EXE_crcbl");

    // 1. Scaffold.
    let created = run(
        "crcbl new",
        Command::new(crcbl)
            .current_dir(temporary.path())
            .args(["new", "mygame", "--engine"])
            .arg(&engine)
            .arg("--json"),
    );
    let json = String::from_utf8_lossy(&created.stdout).into_owned();
    assert!(json.starts_with(r#"{"ok":true,"command":"new""#), "{json}");

    let project = temporary.path().join("mygame");
    let target = temporary.path().join("target");

    // 2. It compiles. This is the assertion the whole file exists for.
    run(
        "cargo build",
        scaffold_cargo(&project, &target).arg("build"),
    );

    // 3. It is formatted and lint-clean, because the generated CI checks both
    //    and a scaffold whose own CI fails on the first push is a broken
    //    scaffold. `rustfmt` directly rather than `cargo fmt`, which would also
    //    reformat the engine reached through the path dependency.
    run(
        "rustfmt --check",
        Command::new("rustfmt")
            .args(["--check", "--edition", "2024"])
            .arg(project.join("src/main.rs")),
    );
    run(
        "cargo clippy",
        scaffold_cargo(&project, &target).args(["clippy", "--all-targets", "--", "-D", "warnings"]),
    );

    // 4. `crcbl run --headless` — the loop, with no display anywhere.
    let ran = run(
        "crcbl run --headless",
        Command::new(crcbl)
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", &target)
            .args(["run", "--headless", "--", "--frames", "30"]),
    );
    let output = String::from_utf8_lossy(&ran.stdout).into_owned();
    assert!(
        output.contains("mygame: 30 frames"),
        "the game ran its frame budget:\n{output}"
    );

    // 5. `crcbl build`, machine-readable.
    let built = run(
        "crcbl build --json",
        Command::new(crcbl)
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", &target)
            .args(["build", "--json"]),
    );
    let json = String::from_utf8_lossy(&built.stdout).into_owned();
    assert!(json.contains(r#""ok":true"#), "{json}");
    assert!(json.contains(r#""cargo_exit_code":0"#), "{json}");
}
