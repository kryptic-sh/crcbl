//! `--set` through the real binary: the shared argument path, the front end
//! that installs the launch layers, and the frame they reach.
//!
//! Lantern is the sample here because its summary line prints the effects its
//! frames were drawn with, resolved — so a settings key reaching the renderer
//! is something stdout says, not something a test has to reach inside for.
//! `--headless` and the null backend are pinned, as `apps/breakout`'s headless
//! tests pin them: without them the run opens a real window, and reads the
//! settings file in whichever home directory it runs in.

use std::process::{Command, Output};

/// Lantern headless on the null backend for a few frames, with `args` after.
///
/// `CRCBL_LOG=warn` is pinned so whether an unknown key's warning reaches
/// stderr does not depend on the developer's environment.
fn lantern(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lantern"))
        .args(["--backend", "null", "--headless", "--frames", "4"])
        .args(args)
        .env("CRCBL_LOG", "warn")
        .output()
        .expect("the lantern binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// **A sample started with `--set` reads the override**: the player's switch
/// for shadows, given on the command line, takes them out of the frame — and
/// the same run without it keeps them.
#[test]
fn a_set_on_the_command_line_reaches_the_frame() {
    let plain = lantern(&[]);
    assert_eq!(plain.status.code(), Some(0), "{}", stderr(&plain));
    assert!(
        stdout(&plain).contains("effects shadows ao ssr vfog cmaa2"),
        "{}",
        stdout(&plain)
    );

    let set = lantern(&["--set", "engine.video.shadows=false"]);
    assert_eq!(set.status.code(), Some(0), "{}", stderr(&set));
    assert!(
        stdout(&set).contains("effects ao ssr vfog cmaa2"),
        "{}",
        stdout(&set)
    );
}

/// **A malformed value is refused by name, with the bad-arguments exit code**,
/// before anything opens.
#[test]
fn a_malformed_set_is_refused_by_name_before_the_run() {
    let refused = lantern(&["--set", "engine.video.shadows=off"]);
    assert_eq!(refused.status.code(), Some(2));
    let line = stderr(&refused)
        .lines()
        .find(|line| line.starts_with("lantern: "))
        .expect("a refused run says why")
        .to_owned();
    assert!(line.contains("engine.video.shadows"), "{line}");
    assert!(line.contains("not a TOML value"), "{line}");
    assert!(stdout(&refused).is_empty(), "the run never started");
}

/// **An unknown key warns, naming it, and the run goes on.**
#[test]
fn an_unknown_key_warns_by_name_and_runs() {
    let warned = lantern(&["--set", "engine.video.shadow=false"]);
    assert_eq!(warned.status.code(), Some(0), "{}", stderr(&warned));
    assert!(
        stderr(&warned).contains("--set engine.video.shadow names a key nothing defines"),
        "{}",
        stderr(&warned)
    );
    assert!(
        stdout(&warned).contains("effects shadows ao ssr vfog cmaa2"),
        "a key nothing reads changed nothing: {}",
        stdout(&warned)
    );
}
