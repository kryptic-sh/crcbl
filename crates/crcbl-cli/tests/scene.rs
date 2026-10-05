//! `crcbl scene` and `crcbl edit`, run as the shipped binary against temporary
//! copies of towers' committed field.
//!
//! Each edit is held to what the editor's own document makes of the same
//! edit: the binary edits one copy, this test applies the command to a second
//! copy through `crcbl::scene_edit::Document` in process, and the two
//! directories must then hold the same bytes. An undo run as a separate
//! process must put the files back byte for byte — the history kept beside
//! the scene is the only thing carrying the edit between the two runs.
//!
//! Nothing here touches the committed field: every test copies it first.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crcbl::reflect::Value;
use crcbl::scene::edit::EditCommand;
use crcbl::scene::scn::{EntityName, SceneEntityId};
use crcbl::scene_edit::{Document, EditError, HISTORY, SCENE_LOCK, lock_scene};

/// Plot 4, `entry`, in towers' field.
const ENTRY: SceneEntityId = SceneEntityId(4);

/// The exit code of each refusal: `REFUSED_BASE` plus the protocol's code.
const UNKNOWN_ENTITY: i32 = 14;
const UNKNOWN_SYSTEM: i32 = 15;
const UNKNOWN_PATH: i32 = 16;
const INVALID: i32 = 17;
const NOTHING_TO_UNDO: i32 = 19;
const NOTHING_TO_REDO: i32 = 20;
/// A history refused.
const HISTORY_REFUSED: i32 = 3;
/// A scene another program holds the lock on.
const LOCKED: i32 = 4;

/// The engine checkout these tests run inside.
fn engine_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the CLI lives two levels below the workspace root")
        .to_path_buf()
}

/// A private directory that cleans itself up, as `tests/cli.rs` has: this
/// crate takes no `tempfile` for a test helper.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "crcbl-cli-scene-{label}-{}-{:?}",
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

/// A copy of towers' committed field under `temp`, named `name`.
fn field_copy(temp: &TempDir, name: &str) -> PathBuf {
    let from = engine_root().join("apps/towers/assets/scenes/field.scn");
    let to = temp.path().join(name);
    copy_tree(&from, &to);
    to
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a fresh directory");
    for entry in std::fs::read_dir(from).expect("a readable directory") {
        let entry = entry.expect("a directory entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("a copied file");
        }
    }
}

/// Every file under `dir` but the history and the lock, keyed by its path
/// relative to it.
fn scene_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = every_file(dir);
    files.remove(HISTORY);
    files
}

/// Every file under `dir` but the lock — the scene and its history — keyed
/// by its path relative to it. The lock file holds a process id, which no
/// two runs share, and Windows refuses a read of it while it is held.
fn every_file(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect(dir, dir, &mut files);
    files
}

fn collect(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if path.file_name() != Some(std::ffi::OsStr::new(SCENE_LOCK)) {
            let key = path
                .strip_prefix(root)
                .expect("under the root")
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(key, std::fs::read(&path).expect("a readable file"));
        }
    }
}

/// Runs the binary.
fn crcbl(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crcbl"))
        .args(args)
        .output()
        .expect("the crcbl binary runs")
}

/// `crcbl scene <verb> <dir> <rest…>`.
fn scene(verb: &str, dir: &Path, rest: &[&str]) -> Output {
    let dir = dir.to_str().expect("a temporary path is text");
    let mut args = vec!["scene", verb, dir];
    args.extend_from_slice(rest);
    crcbl(&args)
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("not killed by a signal")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Exit 0, or a panic showing what the binary said.
fn ok(output: &Output) {
    assert_eq!(
        code(output),
        0,
        "stdout: {}\nstderr: {}",
        stdout(output),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The same scene, opened in process through the editor's vocabulary.
fn document(dir: &Path) -> Document {
    Document::open_dir(dir, crcbl_editor::scene::vocabulary()).expect("the field opens")
}

/// A write of `value` into `path` of `entity`'s component in `system`.
fn set(entity: SceneEntityId, system: &str, path: &str, value: Value) -> EditCommand {
    EditCommand::SetProperty {
        entity,
        system: system.to_owned(),
        path: path.to_owned(),
        value,
    }
}

/// **A move through the CLI writes what the editor's document writes**, and
/// an undo run as another process puts every byte back; a redo puts the move
/// back. The towers half of `docs/plan/08-editor.md`'s exit criterion: the map
/// changed from the CLI with its history intact.
#[test]
fn a_plot_moved_from_the_cli_undoes_byte_for_byte_in_another_run() {
    let temp = TempDir::new("move");
    let dir = field_copy(&temp, "field.scn");
    let expected_dir = field_copy(&temp, "expected.scn");
    let before = scene_files(&dir);

    ok(&scene("move", &dir, &["4", "1.5", "0", "-2.25"]));
    let moved = scene_files(&dir);
    assert_ne!(moved, before, "the move changed nothing");

    let mut expected = document(&expected_dir);
    expected
        .apply(EditCommand::Batch(vec![
            set(ENTRY, "plots", "position.0", Value::Float(1.5)),
            set(ENTRY, "plots", "position.1", Value::Float(0.0)),
            set(ENTRY, "plots", "position.2", Value::Float(-2.25)),
        ]))
        .expect("a plot moves");
    expected.save().expect("the copy saves");
    assert_eq!(moved, scene_files(&expected_dir), "not the document's move");

    ok(&scene("undo", &dir, &[]));
    assert_eq!(scene_files(&dir), before, "the undo left bytes behind");

    ok(&scene("redo", &dir, &[]));
    assert_eq!(scene_files(&dir), moved, "the redo is not the move");
}

/// **A spawn with its fields set is one edit**: the document's spawn and
/// writes, one undo away from the field as it was.
#[test]
fn a_spawn_with_fields_is_the_documents_spawn_and_one_undo() {
    let temp = TempDir::new("spawn");
    let dir = field_copy(&temp, "field.scn");
    let expected_dir = field_copy(&temp, "expected.scn");
    let before = scene_files(&dir);

    let output = scene(
        "spawn",
        &dir,
        &[
            "plots",
            "--set",
            "position.0=-3.5",
            "--set",
            "label=\"north\"",
            "--json",
        ],
    );
    ok(&output);
    let json = stdout(&output);
    assert!(json.contains(r#""entity":9"#), "{json}");
    assert!(
        json.contains(r#""history":{"position":1,"length":1}"#),
        "one entry: {json}"
    );

    let mut expected = document(&expected_dir);
    let id = expected.add_entity("plots").expect("a plot spawns");
    assert_eq!(id, SceneEntityId(9));
    expected
        .apply(EditCommand::Batch(vec![
            set(id, "plots", "position.0", Value::Float(-3.5)),
            set(id, "plots", "label", Value::Text("north".to_owned())),
        ]))
        .expect("a plot's fields");
    expected.save().expect("the copy saves");
    assert_eq!(scene_files(&dir), scene_files(&expected_dir));

    ok(&scene("undo", &dir, &[]));
    assert_eq!(
        scene_files(&dir),
        before,
        "the spawn took more than one undo"
    );
}

/// **A set and a delete are the document's**, and each undoes in a run of
/// its own.
#[test]
fn a_set_and_a_delete_are_the_documents_and_undo_in_turn() {
    let temp = TempDir::new("set-delete");
    let dir = field_copy(&temp, "field.scn");
    let expected_dir = field_copy(&temp, "expected.scn");
    let before = scene_files(&dir);

    ok(&scene("set", &dir, &["#7", "label", "\"centre\""]));
    let labelled = scene_files(&dir);
    ok(&scene("delete", &dir, &["5"]));

    let mut expected = document(&expected_dir);
    expected
        .apply(set(
            SceneEntityId(7),
            "plots",
            "label",
            Value::Text("centre".to_owned()),
        ))
        .expect("a label");
    expected.delete(&[SceneEntityId(5)]).expect("a plot goes");
    expected.save().expect("the copy saves");
    assert_eq!(scene_files(&dir), scene_files(&expected_dir));

    ok(&scene("undo", &dir, &[]));
    assert_eq!(scene_files(&dir), labelled, "the delete did not come back");
    ok(&scene("undo", &dir, &[]));
    assert_eq!(scene_files(&dir), before, "the label did not go back");
    assert_eq!(code(&scene("undo", &dir, &[])), NOTHING_TO_UNDO);
}

/// **Each refusal exits with its own code** and leaves the files alone: no
/// such entity, a value its field refuses, no such field — and an undo or a
/// redo with nothing to walk.
#[test]
fn refusals_exit_with_their_own_codes_and_change_nothing() {
    let temp = TempDir::new("refusals");
    let dir = field_copy(&temp, "field.scn");
    let before = scene_files(&dir);

    for (args, expected, case) in [
        (
            vec!["999", "label", "\"x\""],
            UNKNOWN_ENTITY,
            "an unknown id",
        ),
        (
            vec!["nobody", "label", "\"x\""],
            UNKNOWN_ENTITY,
            "an unknown name",
        ),
        (
            vec!["4", "position.0", "east"],
            INVALID,
            "a word for a number",
        ),
        (
            vec!["4", "position.1", "2.0"],
            INVALID,
            "a plot off the ground",
        ),
        (
            vec!["4", "height", "1.0"],
            UNKNOWN_PATH,
            "a field plots lack",
        ),
        (
            vec!["4", "label", "\"x\"", "--system", "nowhere"],
            18,
            "a system the plot is not in",
        ),
    ] {
        let output = scene("set", &dir, &args);
        assert_eq!(code(&output), expected, "{case}: {output:?}");
        assert_eq!(scene_files(&dir), before, "{case} changed the field");
    }
    assert_eq!(
        code(&scene("spawn", &dir, &["nowhere"])),
        UNKNOWN_SYSTEM,
        "a system the vocabulary lacks"
    );
    assert_eq!(code(&scene("undo", &dir, &[])), NOTHING_TO_UNDO);
    assert_eq!(code(&scene("redo", &dir, &[])), NOTHING_TO_REDO);
    assert_eq!(scene_files(&dir), before);
    assert!(
        !dir.join(HISTORY).exists(),
        "a refused edit wrote a history"
    );
}

/// **A refusal under `--json` names its code** on stdout, beside the
/// message, with `"ok":false`.
#[test]
fn a_refusal_under_json_carries_its_code_and_name() {
    let temp = TempDir::new("refusal-json");
    let dir = field_copy(&temp, "field.scn");
    let output = scene("delete", &dir, &["999", "--json"]);
    assert_eq!(code(&output), UNKNOWN_ENTITY);
    let json = stdout(&output);
    for field in [
        r#""ok":false"#,
        r#""command":"scene""#,
        r#""verb":"delete""#,
        r#""refusal":4"#,
        r#""reason":"unknown entity""#,
    ] {
        assert!(json.contains(field), "{field} missing from {json}");
    }
}

/// **A history whose bytes were changed is refused**, and nothing is
/// replayed from it: the files stay as the edit left them.
#[test]
fn a_tampered_history_is_refused_and_nothing_replays() {
    let temp = TempDir::new("tampered");
    let dir = field_copy(&temp, "field.scn");
    ok(&scene("move", &dir, &["4", "1.5", "0", "-2.25"]));
    let moved = scene_files(&dir);

    let path = dir.join(HISTORY);
    let mut bytes = std::fs::read(&path).expect("a history");
    // The position: one entry applied, made none, so a history read without
    // its checksum would answer "nothing to undo" rather than be refused.
    let position = 8 + 2 + 32;
    assert_eq!(bytes[position], 1, "the layout moved");
    bytes[position] = 0;
    std::fs::write(&path, &bytes).expect("written");

    let output = scene("undo", &dir, &[]);
    assert_eq!(code(&output), HISTORY_REFUSED, "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("checksum"),
        "{output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("; remove it to start a new one"),
        "the refusal does not say how to start over: {output:?}"
    );
    assert_eq!(scene_files(&dir), moved, "something replayed");
}

/// **A history written beside other files is refused**: the scene changed
/// since the edit — here by the editor's document, as a person in the GUI
/// would — so the history's inverses are not the scene's.
#[test]
fn a_history_beside_a_changed_scene_is_refused() {
    let temp = TempDir::new("changed");
    let dir = field_copy(&temp, "field.scn");
    ok(&scene("move", &dir, &["4", "1.5", "0", "-2.25"]));

    let mut elsewhere = document(&dir);
    elsewhere
        .apply(set(
            SceneEntityId(6),
            "plots",
            "label",
            Value::Text("far".to_owned()),
        ))
        .expect("a label");
    elsewhere.save().expect("the field saves");
    let changed = scene_files(&dir);

    let output = scene("undo", &dir, &[]);
    assert_eq!(code(&output), HISTORY_REFUSED, "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("changed"),
        "{output:?}"
    );
    assert_eq!(scene_files(&dir), changed, "the stale inverse was replayed");
}

/// **`list --json` is the documented shape**: the scene's name, every system
/// with its entities, and every entity with its systems.
#[test]
fn list_under_json_names_every_system_and_entity() {
    let temp = TempDir::new("list");
    let dir = field_copy(&temp, "field.scn");
    let output = scene("list", &dir, &["--json"]);
    ok(&output);
    let json = stdout(&output);
    for field in [
        r#"{"ok":true,"command":"scene","verb":"list","scene":"field","systems":["#,
        r#"{"name":"waypoints","entities":[0,1,2,3]}"#,
        r#"{"name":"plots","entities":[4,5,6,7,8]}"#,
        r#"{"id":4,"name":null,"systems":["plots"]}"#,
    ] {
        assert!(json.contains(field), "{field} missing from {json}");
    }
    let human = stdout(&scene("list", &dir, &[]));
    assert!(human.contains("scene `field`"), "{human}");
    assert!(human.contains("  #4"), "{human}");
}

/// **`query` reads an entity by name or id, and a system's every row**, each
/// field as the chunk file spells it.
#[test]
fn query_reads_an_entity_by_name_and_a_system() {
    let temp = TempDir::new("query");
    let dir = field_copy(&temp, "field.scn");
    let mut named = document(&dir);
    named
        .apply(EditCommand::Rename {
            entity: ENTRY,
            name: Some(EntityName::new("Entry").expect("a name")),
        })
        .expect("a rename");
    named.save().expect("the field saves");

    let output = scene("query", &dir, &["Entry", "--json"]);
    ok(&output);
    let json = stdout(&output);
    assert!(
        json.contains(
            r#""entity":{"id":4,"name":"Entry","systems":["plots"],"components":[{"system":"plots","fields":[{"path":"label","value":"\"entry\""},{"path":"position.0","value":"-6.0"}"#
        ),
        "{json}"
    );

    let human = stdout(&scene("query", &dir, &["plots"]));
    assert!(human.contains("system `plots`"), "{human}");
    assert!(
        human.contains("#8\n  plots\n    label = \"gate\""),
        "{human}"
    );
}

/// **`crcbl edit` applies every `-e` in order as an entry each**, saving
/// once — and one refused saves nothing, exiting with its code.
#[test]
fn edit_applies_each_command_as_an_entry_and_a_refusal_saves_nothing() {
    let temp = TempDir::new("edit");
    let dir = field_copy(&temp, "field.scn");
    let before = scene_files(&dir);
    let dir_text = dir.to_str().expect("a temporary path is text");

    let output = crcbl(&[
        "edit",
        dir_text,
        "-e",
        "move 4 1.5 0 -2.25",
        "-e",
        "set 4 label \"far gate\"",
        "--json",
    ]);
    ok(&output);
    let json = stdout(&output);
    assert!(json.contains(r#""applied":2"#), "{json}");
    assert!(
        json.contains(r#""history":{"position":2,"length":2}"#),
        "{json}"
    );
    let query = stdout(&scene("query", &dir, &["4"]));
    assert!(query.contains("label = \"far gate\""), "{query}");

    let edited = scene_files(&dir);
    let refused = crcbl(&["edit", dir_text, "-e", "undo", "-e", "delete 999", "--json"]);
    assert_eq!(code(&refused), UNKNOWN_ENTITY, "{refused:?}");
    let json = stdout(&refused);
    assert!(json.contains(r#""command":"edit""#), "{json}");
    assert!(
        json.contains(r#""failed":1"#),
        "the second -e was refused: {json}"
    );
    assert_eq!(scene_files(&dir), edited, "a refused run saved its undo");

    ok(&crcbl(&["edit", dir_text, "-e", "undo", "-e", "undo"]));
    assert_eq!(scene_files(&dir), before);
}

/// The scene at `dir` opened as the editor opens it: locked first, then read
/// with its history, the document holding the lock until it is dropped.
fn editor_holding(dir: &Path) -> Document {
    let lock = lock_scene(dir).expect("nobody else holds the scene");
    Document::open_locked(lock, crcbl_editor::scene::vocabulary()).expect("the field opens")
}

/// **An edit while the editor holds the scene exits locked and changes
/// nothing** — not the scene, not its history — under `crcbl scene` and
/// `crcbl edit` alike, and `--json` says so; a read takes no lock and goes
/// on. Once the editor lets the scene go, the same edit lands.
#[test]
fn an_edit_while_the_editor_holds_the_scene_is_refused_until_it_closes() {
    let temp = TempDir::new("locked");
    let dir = field_copy(&temp, "field.scn");
    ok(&scene("move", &dir, &["4", "1.5", "0", "-2.25"]));
    let before = every_file(&dir);

    let editor = editor_holding(&dir);
    let output = scene("move", &dir, &["4", "3", "0", "1"]);
    assert_eq!(code(&output), LOCKED, "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(SCENE_LOCK),
        "the refusal does not name the lock: {output:?}"
    );
    let dir_text = dir.to_str().expect("a temporary path is text");
    let output = crcbl(&["edit", dir_text, "-e", "undo"]);
    assert_eq!(code(&output), LOCKED, "{output:?}");
    let output = scene("undo", &dir, &["--json"]);
    assert_eq!(code(&output), LOCKED, "{output:?}");
    assert!(stdout(&output).contains("\"ok\":false"), "{output:?}");
    assert_eq!(every_file(&dir), before, "a refused run changed the scene");
    ok(&scene("list", &dir, &[]));

    drop(editor);
    ok(&scene("move", &dir, &["4", "3", "0", "1"]));
    assert_ne!(
        scene_files(&dir),
        scene_files_of(&before),
        "the move did not land"
    );
}

/// [`scene_files`] of a set [`every_file`] took.
fn scene_files_of(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    let mut files = files.clone();
    files.remove(HISTORY);
    files
}

/// **A lock file a crashed holder left blocks nobody**: a file the operating
/// system released, as it releases one when its process ends, is taken over
/// by the next run, which edits and saves.
#[test]
fn a_lock_file_left_by_a_dead_process_does_not_block() {
    let temp = TempDir::new("stale");
    let dir = field_copy(&temp, "field.scn");
    std::fs::write(
        dir.join(SCENE_LOCK),
        "editor (process 4242)
",
    )
    .expect("written");
    let before = scene_files(&dir);

    ok(&scene("move", &dir, &["4", "1.5", "0", "-2.25"]));
    assert_ne!(scene_files(&dir), before, "the move did not land");
    assert!(
        lock_scene(&dir).is_ok(),
        "the run kept the lock past its end"
    );
}

/// **The editor cannot take a scene a run holds**: while one holds it, the
/// editor's open is refused, which is the other half of the same lock. Held
/// here by an edit run's own call, as `crcbl scene` makes it.
#[test]
fn the_editor_cannot_open_a_scene_another_holder_has() {
    let temp = TempDir::new("held");
    let dir = field_copy(&temp, "field.scn");
    let run = lock_scene(&dir).expect("free");
    match lock_scene(&dir)
        .and_then(|lock| Document::open_locked(lock, crcbl_editor::scene::vocabulary()))
    {
        Err(EditError::Locked { dir: held, .. }) => assert_eq!(held, dir),
        other => panic!("the editor opened a held scene: {other:?}"),
    }
    drop(run);
    editor_holding(&dir);
}

/// **A served scene is another process's to fetch, and locked until
/// `quit`.** `crcbl edit --serve` says where it serves — loopback, by
/// default — and a client of the edit protocol in this process fetches the
/// scene from it, byte for byte the files on disk; meanwhile a `crcbl scene`
/// edit exits locked. `quit` typed at its standard input ends it with exit
/// 0, and the same edit then lands.
#[test]
fn a_served_scene_is_fetched_from_another_process_and_locked_until_quit() {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::net::SocketAddr;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use crcbl::scene_edit::SceneFollower;
    use crcbl::scene_edit::serve::{EDIT_PROTOCOL_ID, EDIT_TICK_HZ, edit_compatibility};

    /// Long past a fetch of towers' field over loopback.
    const PATIENCE: Duration = Duration::from_secs(10);

    let temp = TempDir::new("served");
    let dir = field_copy(&temp, "field.scn");
    let before = scene_files(&dir);
    let dir_text = dir.to_str().expect("a temporary path is text");
    let mut server = Command::new(env!("CARGO_BIN_EXE_crcbl"))
        .args(["edit", dir_text, "--serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the crcbl binary runs");
    let mut said = BufReader::new(server.stdout.take().expect("piped"));
    let mut serving = String::new();
    said.read_line(&mut serving).expect("a line");
    let addr: SocketAddr = serving
        .strip_prefix(&format!("edit: serving `{dir_text}` on UDP "))
        .and_then(|rest| rest.split(',').next())
        .and_then(|addr| addr.parse().ok())
        .unwrap_or_else(|| panic!("not where it serves: {serving}"));
    assert!(
        addr.ip().is_loopback(),
        "served beyond this machine: {addr}"
    );

    let vocabulary = crcbl_editor::scene::vocabulary();
    let transport =
        crcbl::net::udp::UdpTransport::connect(addr, EDIT_PROTOCOL_ID).expect("a socket");
    let mut client = crcbl::client::Client::new_with_compatibility(
        crcbl::ecs::World::new(),
        transport,
        EDIT_TICK_HZ,
        edit_compatibility(&vocabulary),
        crcbl::net::PlayerId::from_seed(111),
    );
    let mut follower = SceneFollower::new(vocabulary);
    let started = Instant::now();
    while follower.revision().is_none() {
        assert!(started.elapsed() < PATIENCE, "the scene never came");
        client.update(started.elapsed());
        follower.update(&mut client, started.elapsed());
        std::thread::sleep(Duration::from_millis(1));
    }
    let fetched: BTreeMap<String, Vec<u8>> = follower
        .document_mut()
        .expect("a copy")
        .files()
        .expect("a copy saves")
        .into_iter()
        .map(|(key, text)| (key, text.into_bytes()))
        .collect();
    assert_eq!(fetched, before, "the fetched scene is not the files");

    let label = ["4", "label", "\"gate\""];
    assert_eq!(code(&scene("set", &dir, &label)), LOCKED);
    server
        .stdin
        .take()
        .expect("piped")
        .write_all(b"quit\n")
        .expect("the console reads");
    let ended = Instant::now();
    let status = loop {
        if let Some(status) = server.try_wait().expect("a child to wait on") {
            break status;
        }
        if ended.elapsed() > PATIENCE {
            server.kill().expect("a running child");
            panic!("`quit` did not end the server");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "the quit failed: {status}");
    let mut rest = String::new();
    said.read_to_string(&mut rest)
        .expect("the rest of what it said");
    assert!(rest.contains("0/8 clients, saved"), "{rest}");
    assert_eq!(scene_files(&dir), before, "serving with no edit wrote");
    ok(&scene("set", &dir, &label));
}
