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

/// The GPU backend the scaffolded game is run against.
///
/// `null` by default, which keeps the property `run-cli-e2e.sh` documents: this
/// suite needs no display, no compositor and **no driver**. A generated project
/// goes through `crcbl::backend`'s real registry, and that registry deliberately
/// never falls back to null — so on a machine with no driver at all, which is
/// every stock CI runner, the scaffold has to be told.
///
/// CI sets `CRCBL_CLI_E2E_BACKEND=vk` with lavapipe installed, which is what
/// puts the template's render graph in front of an actual driver rather than a
/// recorder. Set it locally to do the same.
fn gpu_backend() -> String {
    std::env::var("CRCBL_CLI_E2E_BACKEND").unwrap_or_else(|_| "null".to_string())
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

    // 4. Its own tests pass, so a scaffold whose menu ids collided or whose
    //    input took a shortcut fails here rather than in the first game
    //    somebody builds on it.
    //
    //    `running 0 tests` is checked for by name: `run` already fails on a
    //    non-zero exit, and a suite with nothing in it exits zero. That is the
    //    same trap `run-cli-e2e.sh` guards this file against, one level down.
    let tested = run("cargo test", scaffold_cargo(&project, &target).arg("test"));
    let tests = String::from_utf8_lossy(&tested.stdout).into_owned();
    assert!(
        tests.contains("test result: ok.") && !tests.contains("running 0 tests"),
        "the scaffold's own tests did not run:\n{tests}"
    );

    // 5. `crcbl run --headless` — the loop, with no display anywhere.
    let backend = gpu_backend();
    let ran = run(
        "crcbl run --headless",
        Command::new(crcbl)
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", &target)
            .args(["run", "--headless", "--"])
            .args(["--frames", "30", "--backend", &backend]),
    );
    let output = String::from_utf8_lossy(&ran.stdout).into_owned();
    assert!(
        output.contains("mygame: 30 frames"),
        "the game ran its frame budget on the {backend} backend:\n{output}"
    );

    // 6. `crcbl build`, machine-readable.
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
