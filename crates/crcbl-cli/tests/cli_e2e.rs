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

/// Puts a scaffolded run in front of the validation layer, and makes what the
/// layer says matter.
///
/// Three variables, none of which the template knows about — this is the
/// harness asking the engine underneath it to be strict:
///
/// * `CRCBL_LOG=info` so the messenger's own lines reach the child's stderr at
///   all. Everything below reads that stream, and the default filter is not
///   this test's to assume.
/// * `CRCBL_VK_VALIDATION=1` so the layer is requested even from a release
///   build, rather than relying on this being a debug one.
/// * `CRCBL_VK_VALIDATION_FATAL=1` so a specification violation reaches
///   [`crcbl_hal::Device::take_error`] and the child *exits non-zero*. Without
///   it a violating run logs and exits 0, which is what every scaffold run did
///   until now: advertised as validating, unable to fail.
///
/// Inert on the null backend, which opens no Vulkan instance, so this is set
/// unconditionally rather than behind a branch on `CRCBL_CLI_E2E_BACKEND`.
fn validating(command: &mut Command) -> &mut Command {
    command
        .env("CRCBL_LOG", "info")
        .env("CRCBL_VK_VALIDATION", "1")
        .env("CRCBL_VK_VALIDATION_FATAL", "1")
}

/// The complaint lines the debug messenger wrote, in order.
///
/// The level *and* the module *and* the callback's own `vk <kind>:` prefix,
/// which is what keeps `crcbl_vk::device`'s teardown warning — a different
/// question, asked elsewhere — out of this one. Errors and warnings both, which
/// is where `crcbl-vk`'s own `ValidationReport::assert_clean` draws the line;
/// the messenger subscribes to no other severity, so there is nothing else to
/// filter out.
fn validation_complaints(log: &str) -> Vec<&str> {
    log.lines()
        .filter(|line| {
            line.contains("crcbl_vk::debug] vk ")
                && (line.contains("ERROR") || line.contains("WARN"))
        })
        .collect()
}

/// Fails when the layer said anything about a run, or was never there to say it.
///
/// The second half is not pedantry. A log with no validation errors in it is
/// exactly what a run with no messenger produces, so on its own the complaint
/// scan is a green light wired to nothing; `crcbl-vk` prints the "validation
/// enabled" line only once the messenger really exists. The three shell e2e
/// harnesses carry this same pair of questions against the logs of the samples
/// they run.
///
/// Skipped on the null backend, which has no layer to load.
fn assert_validation_clean(what: &str, backend: &str, output: &Output) {
    if backend != "vk" {
        return;
    }
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(
        log.contains("crcbl-vk: validation enabled ("),
        "{what}: ran with CRCBL_VK_VALIDATION=1 and never loaded the layer, so a \
         clean log here proves nothing. Install VK_LAYER_KHRONOS_validation \
         (Arch: vulkan-validation-layers, Debian/Ubuntu: vulkan-validationlayers) \
         — crcbl-vk warns by name when it is missing:\n{log}"
    );
    assert!(
        !log.contains("a panic escaped the Vulkan debug messenger callback"),
        "{what}: lost validation messages — a panic escaped the messenger \
         callback, so the scan below cannot see what the layer said:\n{log}"
    );
    let complaints = validation_complaints(&log);
    assert!(
        complaints.is_empty(),
        "{what}: the validation layer complained about the scaffolded run:\n{}",
        complaints.join("\n")
    );
}

/// The pass that proves [`assert_validation_clean`] can fail.
///
/// Both of its questions are answered by *absence* — no complaint lines, and
/// one line that has to be there — and an absence is what a run that never
/// reached the layer produces too. So this runs the scaffold once more with
/// `CRCBL_VK_VALIDATION_SELF_TEST=1`, which asks a debug `crcbl-vk` to put one
/// synthetic `ERROR` through `vkSubmitDebugUtilsMessageEXT` as the instance
/// opens. Everything downstream of the callback is then the real path: the sink
/// records it, `CRCBL_VK_VALIDATION_FATAL=1` turns it into a
/// `Device::take_error`, the engine drains that at the top of the first frame,
/// and the child dies.
///
/// Both halves are asserted, because either alone is satisfiable by the wrong
/// thing: a child that failed for an unrelated reason, or a log line from a
/// child that exited 0 anyway.
///
/// What it does *not* settle is that the layer would have caught a real
/// violation: a message put through `vkSubmitDebugUtilsMessageEXT` is delivered
/// whatever the layer's checks are set to. That is
/// [`self_test_the_layer_is_checking`]'s question, and it needs its own run.
fn self_test_the_validation_gate(crcbl: &str, project: &Path, target: &Path, backend: &str) {
    if backend != "vk" {
        return;
    }
    let injected = validating(
        Command::new(crcbl)
            .current_dir(project)
            .env("CARGO_TARGET_DIR", target),
    )
    .env("CRCBL_VK_VALIDATION_SELF_TEST", "1")
    .args(["run", "--headless", "--"])
    .args(["--frames", "1", "--backend", backend, "--size", "320x240"])
    .output()
    .expect("crcbl run (self-test): could not start");
    let log = String::from_utf8_lossy(&injected.stderr);
    assert!(
        log.contains("CRCBL-VALIDATION-SELF-TEST"),
        "the injected validation message never reached the scaffold's log, so \
         the checks that read that log are reading nothing. A release build \
         cannot be asked for the injection and says so; this one was built by \
         `crcbl run`, which builds debug:\n{log}"
    );
    assert!(
        !injected.status.success(),
        "the scaffold survived an injected validation ERROR with \
         CRCBL_VK_VALIDATION_FATAL=1 set, so a real violation would not fail it \
         either:\n{log}"
    );
    assert!(
        !validation_complaints(&log).is_empty(),
        "the injected message reached the log but not in the shape \
         validation_complaints matches, so the scan above is reading past real \
         complaints too:\n{log}"
    );
}

/// The pass that proves the layer is **checking**, not merely loaded.
///
/// Everything above is satisfied by a layer that loads, prints its line and
/// validates nothing — measured on 1.4.357 with
/// `VK_KHRONOS_VALIDATION_VALIDATE_CORE=false`. So this asks
/// `CRCBL_VK_VALIDATION_PROVOKE=1` for one deliberate out-of-bounds
/// `vkCmdCopyBuffer`, recorded and destroyed without ever being submitted, at
/// the scaffold's first present; only a core check reports it.
///
/// **Its own run rather than the self-test's**, which is not tidiness: with
/// `CRCBL_VK_VALIDATION_FATAL=1` set on both, the injected message kills the
/// child at the first frame's `acquire`, which is before any present has
/// happened — so the provocation would never fire and this would assert on a
/// run that never reached it. Two frames for the same reason from the other
/// end: the error is recorded during the first present, and the second
/// `acquire` is what drains it.
///
/// The layer's own complaint, never `crcbl-vk`'s `CRCBL_VK_VALIDATION_PROVOKE
/// records …` line — that line deliberately names neither the entry point nor
/// the VUIDs, because a grep the run's own output can answer proves nothing.
/// The digits of the `VUID-vkCmdCopyBuffer-size-*` pair belong to the layer
/// build and are not named here either.
fn self_test_the_layer_is_checking(crcbl: &str, project: &Path, target: &Path, backend: &str) {
    if backend != "vk" {
        return;
    }
    let provoked = validating(
        Command::new(crcbl)
            .current_dir(project)
            .env("CARGO_TARGET_DIR", target),
    )
    .env("CRCBL_VK_VALIDATION_PROVOKE", "1")
    .args(["run", "--headless", "--"])
    .args(["--frames", "2", "--backend", backend, "--size", "320x240"])
    .output()
    .expect("crcbl run (provocation): could not start");
    let log = String::from_utf8_lossy(&provoked.stderr);
    assert!(
        log.contains("vk validation: VUID-vkCmdCopyBuffer-size-"),
        "the scaffold ran with CRCBL_VK_VALIDATION_PROVOKE=1 and the layer said \
         nothing about the deliberate out-of-bounds copy crcbl-vk recorded at \
         its first present. Only a core check emits that, so this layer is \
         loaded and checking nothing — the state every other validation \
         assertion here reads as success:\n{log}"
    );
    assert!(
        !provoked.status.success(),
        "the layer reported the provoked violation and the scaffold survived it \
         with CRCBL_VK_VALIDATION_FATAL=1 set, so a real violation would not \
         fail it either:\n{log}"
    );
}

/// The size `templates/main.rs.tmpl` asks its window to open at.
///
/// Named here rather than read out of the template: the point of the windowed
/// pass is that the *generated project* opens at the size its own source says,
/// so a check that took the number from the same file the code takes it from
/// would agree with itself no matter what either did.
const SCAFFOLD_WINDOW: &str = "960x720";

/// Whether to run the scaffolded game **windowed**, against a real compositor.
///
/// Off unless `run-cli-e2e.sh` says otherwise, because the rest of this suite
/// deliberately needs no display: `crcbl new` has to keep working on a machine
/// with no window system, and a test that silently required one would be a
/// worse scaffold gate than no test.
///
/// The harness sets it after starting headless sway and exporting
/// `WAYLAND_DISPLAY`, which this process inherits.
fn windowed_pass() -> bool {
    std::env::var("CRCBL_CLI_E2E_WINDOWED").is_ok_and(|value| value == "1")
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
        validating(
            Command::new(crcbl)
                .current_dir(&project)
                .env("CARGO_TARGET_DIR", &target),
        )
        .args(["run", "--headless", "--"])
        .args(["--frames", "30", "--backend", &backend, "--size", "320x240"]),
    );
    assert_validation_clean("crcbl run --headless", &backend, &ran);
    let output = String::from_utf8_lossy(&ran.stdout).into_owned();
    assert!(
        output.contains("mygame: 30 frames"),
        "the game ran its frame budget on the {backend} backend:\n{output}"
    );
    // `--size` is a shared flag, so the generated project has to honour it the
    // way the samples do: the headless offscreen ring renders at the extent
    // named. 320x240 is not the template's own default, so this cannot pass by
    // accident.
    assert!(
        output.contains("at 320x240"),
        "the scaffold ignored --size; the summary reports its extent:\n{output}"
    );

    // 5b. And the two passes that give 5's validation check teeth: that it can
    //     fail at all, and that the layer grading it is checking anything.
    self_test_the_validation_gate(crcbl, &project, &target, &backend);
    self_test_the_layer_is_checking(crcbl, &project, &target, &backend);

    // 6. And the same loop with a window on it, when there is a compositor to
    //    put one on. This is the half `--headless` cannot reach: opening a
    //    real surface, joining it to the device, and getting a swapchain at the
    //    size the window system configured rather than the size we asked for.
    //
    //    A generated project is the first thing anyone runs, and until now
    //    nothing had ever run one windowed.
    if windowed_pass() {
        let ran = run(
            "crcbl run (windowed)",
            validating(
                Command::new(crcbl)
                    .current_dir(&project)
                    .env("CARGO_TARGET_DIR", &target)
                    // The compositor the harness started, not whatever the
                    // developer is logged into: a silent fallback to another
                    // backend would report success for a window nobody asserted
                    // on.
                    .env("CRCBL_SHELL", "wayland"),
            )
            .args(["run", "--"])
            .args(["--frames", "60", "--backend", &backend]),
        );
        assert_validation_clean("crcbl run (windowed)", &backend, &ran);
        let output = String::from_utf8_lossy(&ran.stdout).into_owned();
        assert!(
            output.contains("mygame: 60 frames"),
            "the scaffold did not present its frame budget:\n{output}"
        );
        // The window is the size the template asked for, in the mode it asked
        // for, and it got there through the Wayland backend. A *tiled* window
        // would report the output's size instead, which is why the harness's
        // sway config floats this `app_id`. The tick count is deliberately not
        // asserted: a windowed run reads the real clock, so it is a property of
        // how fast the machine is.
        let expected = format!("on the wayland shell at {SCAFFOLD_WINDOW}, windowed");
        assert!(
            output.contains(&expected),
            "the scaffold did not open the window it asked for; wanted {expected:?}:\n{output}"
        );
    }

    // 7. `crcbl build`, machine-readable.
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
