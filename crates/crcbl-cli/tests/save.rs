//! `crcbl save`, run as the shipped binary over saves written by the
//! container's own [`SaveWriter`] into temporary directories.
//!
//! A refusal is held to the text [`SaveReader::open`] gives for the same file
//! in process — the text a game shows — rather than to a copy of it here.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crcbl::core::TickId;
use crcbl::net::types::SectorId;
use crcbl_store::save::{SaveHeader, SaveReader, SaveWriter, SceneRef, SectorSave};
use crcbl_store::{NativeStorage, StorageSource};

/// `cmp`'s codes, which `save diff` takes.
const SAME: i32 = 0;
const DIFFERENT: i32 = 1;
const TROUBLE: i32 = 2;

/// `list` and `dump`'s, the CLI's own contract.
const OK: i32 = 0;
const FAILED: i32 = 1;

/// A private directory that cleans itself up, as `tests/cli.rs` has: this
/// crate takes no `tempfile` for a test helper.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "crcbl-cli-save-{label}-{}-{:?}",
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

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The last line on stderr: the CLI's own message, after whatever the
/// engine's logger printed first.
fn last_error_line(output: &Output) -> String {
    stderr(output).lines().last().unwrap_or_default().to_owned()
}

fn arg(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 temporary path")
}

/// `text` as the JSON string literal the CLI writes it as — the two escapes
/// a Windows path or a refusal can carry.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', r"\\").replace('"', "\\\""))
}

/// The committed version-2 save, written before version 3 existed.
fn v2_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../crcbl-store/tests/fixtures/save-v2.crb")
}

/// A scene reference whose hash bytes are all different, so a hash printed
/// shifted or reversed is not this one.
fn arena() -> SceneRef {
    SceneRef {
        name: "scenes/arena.scn".to_owned(),
        content_hash: std::array::from_fn(|i| i as u8 + 1),
    }
}

/// The content hash of [`arena`], in hex.
const ARENA_HASH: &str = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";

/// A header with every field set to something no default produces.
fn full_header(tick: u64) -> SaveHeader {
    SaveHeader {
        tick: TickId::from_raw(tick),
        playtime_secs: 3.5,
        engine_version: Some("9.8.7-test".to_owned()),
        scene: Some(arena()),
    }
}

fn sector(x: i64, y: i64, z: i64, data: &[u8]) -> SectorSave {
    SectorSave {
        sector_id: SectorId { x, y, z },
        snapshot_data: data.to_vec(),
    }
}

/// Writes a save at `dir/name` through [`SaveWriter`], returning its path.
fn write_save(dir: &Path, name: &str, header: SaveHeader, sectors: Vec<SectorSave>) -> PathBuf {
    let mut writer = SaveWriter::new(header);
    for sector in sectors {
        writer.add_sector(sector);
    }
    writer
        .write(&NativeStorage::at(dir.to_path_buf()), Path::new(name))
        .expect("a temporary directory takes a save");
    dir.join(name)
}

/// The text the game's reader refuses `dir/name` with.
fn refusal(dir: &Path, name: &str) -> String {
    SaveReader::open(&NativeStorage::at(dir.to_path_buf()), Path::new(name))
        .expect_err("a file the reader refuses")
        .to_string()
}

/// A good save, then copies of it the reader refuses for four different
/// reasons: a format version from a newer engine, a flipped byte, a cut-off
/// file, and a file that is not a save at all.
fn mixed_directory(dir: &Path) {
    let good = write_save(
        dir,
        "good.crb",
        full_header(42),
        vec![sector(0, 0, 0, &[1, 2, 3])],
    );
    let bytes = std::fs::read(good).expect("the save just written");

    let mut newer = bytes.clone();
    newer[8..10].copy_from_slice(&999u16.to_le_bytes());
    std::fs::write(dir.join("newer.crb"), newer).expect("a scratch file");

    let mut corrupt = bytes.clone();
    let last_data = bytes.len() - 33;
    corrupt[last_data] ^= 0xFF;
    std::fs::write(dir.join("corrupt.crb"), corrupt).expect("a scratch file");

    std::fs::write(dir.join("short.crb"), b"CRCBLSVE").expect("a scratch file");
    std::fs::write(dir.join("notes.txt"), b"not a save").expect("a scratch file");
    std::fs::create_dir(dir.join("subdir")).expect("a scratch directory");
}

// ── list ────────────────────────────────────────────────────────────────────

/// **`list` names every file, good and bad**, with a refused file's reason
/// as the game's reader gives it, and does not enter a subdirectory.
#[test]
fn list_names_good_and_bad_files_with_the_readers_reason() {
    let temp = TempDir::new("list");
    let dir = temp.path().join("saves");
    std::fs::create_dir(&dir).expect("a scratch directory");
    mixed_directory(&dir);
    std::fs::copy(v2_fixture(), dir.join("old.crb")).expect("a scratch copy");
    let refused = ["newer.crb", "corrupt.crb", "short.crb", "notes.txt"];

    let human = crcbl(temp.path(), &["save", "list", "--dir", arg(&dir)]);
    assert_eq!(code(&human), OK, "{}", stderr(&human));
    let text = stdout(&human);
    let good = text
        .lines()
        .find(|line| line.contains("good.crb"))
        .unwrap_or_else(|| panic!("good.crb is not listed:\n{text}"));
    for field in ["tick 42", "playtime 3.5 s", "format 3", "engine 9.8.7-test"] {
        assert!(good.contains(field), "`{field}` missing from: {good}");
    }
    for name in refused {
        let line = text
            .lines()
            .find(|line| line.contains(name))
            .unwrap_or_else(|| panic!("{name} is not listed:\n{text}"));
        assert!(
            line.contains(&format!("refused: {}", refusal(&dir, name))),
            "{name}'s reason is not the reader's: {line}"
        );
    }
    let old = text
        .lines()
        .find(|line| line.contains("old.crb"))
        .unwrap_or_else(|| panic!("old.crb is not listed:\n{text}"));
    for field in [
        "tick 9001",
        "format 2 (migrated to 3)",
        "engine not recorded",
    ] {
        assert!(old.contains(field), "`{field}` missing from: {old}");
    }
    assert!(!text.contains("subdir"), "a directory was listed:\n{text}");

    let json = stdout(&crcbl(
        temp.path(),
        &["save", "list", "--dir", arg(&dir), "--json"],
    ));
    assert_eq!(json.lines().count(), 1, "one object: {json}");
    assert!(
        json.starts_with(r#"{"ok":true,"command":"save","action":"list","app":null,"#),
        "{json}"
    );
    assert!(json.contains(r#""dir_exists":true,"count":6,"#), "{json}");
    assert!(json.contains(r#"{"file":"good.crb","size":"#), "{json}");
    assert!(
        json.contains(
            r#""ok":true,"format_version":3,"migrated":false,"tick":42,"playtime_secs":3.5,"engine_version":"9.8.7-test"}"#
        ),
        "{json}"
    );
    assert!(
        json.contains(
            r#"{"file":"old.crb","size":155,"ok":true,"format_version":2,"migrated":true,"tick":9001,"playtime_secs":3723.25,"engine_version":null}"#
        ),
        "{json}"
    );
    for name in refused {
        let size = std::fs::metadata(dir.join(name))
            .expect("a scratch file")
            .len();
        let record = format!(
            r#"{{"file":"{name}","size":{size},"ok":false,"error":{}}}"#,
            quoted(&refusal(&dir, name))
        );
        assert!(
            json.contains(&record),
            "{name}'s record is not {record}: {json}"
        );
    }
    // The refusals are the four different texts they are meant to be, so
    // the check above cannot pass by every file sharing one reason.
    let mut reasons: Vec<String> = refused.iter().map(|name| refusal(&dir, name)).collect();
    reasons.sort();
    reasons.dedup();
    assert_eq!(reasons.len(), refused.len(), "{reasons:?}");
}

/// **A game that has never saved lists nothing and creates nothing.**
#[test]
fn list_of_a_missing_directory_is_an_empty_answer_and_creates_nothing() {
    let temp = TempDir::new("list-missing");
    let dir = temp.path().join("never");
    let output = crcbl(temp.path(), &["save", "list", "--dir", arg(&dir), "--json"]);
    assert_eq!(code(&output), OK);
    let json = stdout(&output);
    assert!(
        json.contains(r#""dir_exists":false,"count":0,"saves":[]"#),
        "{json}"
    );
    assert!(!dir.exists(), "`list` created the directory it read");
}

// ── dump ────────────────────────────────────────────────────────────────────

/// **`dump` shows every header field and every sector**, in both renderings.
#[test]
fn dump_shows_every_header_field_and_the_sector_table() {
    let temp = TempDir::new("dump");
    let file = write_save(
        temp.path(),
        "full.crb",
        full_header(77),
        vec![sector(5, -6, 7, &[9, 8, 7]), sector(0, 0, 0, &[])],
    );
    let size = std::fs::metadata(&file).expect("the save").len();
    // SHA-256 of the bytes 09 08 07, and of no bytes at all.
    let digest = crcbl::shaders::sha256::sha256_hex(&[9, 8, 7]);
    let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    let human = crcbl(temp.path(), &["save", "dump", arg(&file)]);
    assert_eq!(code(&human), OK, "{}", stderr(&human));
    let text = stdout(&human);
    for line in [
        format!("{}: {size} bytes", file.display()),
        "format version  3".to_owned(),
        "tick            77".to_owned(),
        "playtime        3.5 s".to_owned(),
        "engine version  9.8.7-test".to_owned(),
        format!("scene           scenes/arena.scn, content sha256 {ARENA_HASH}"),
        "sectors         2".to_owned(),
        format!("  (5, -6, 7)  3 bytes  sha256 {digest}"),
        format!("  (0, 0, 0)  0 bytes  sha256 {empty}"),
    ] {
        assert!(
            text.lines().any(|shown| shown == line),
            "`{line}` missing:\n{text}"
        );
    }

    let json = stdout(&crcbl(temp.path(), &["save", "dump", arg(&file), "--json"]));
    let expected = format!(
        r#"{{"ok":true,"command":"save","action":"dump","file":{},"size":{size},"format_version":3,"current_format_version":3,"migrated":false,"tick":77,"playtime_secs":3.5,"engine_version":"9.8.7-test","scene":{{"name":"scenes/arena.scn","content_hash":"{ARENA_HASH}"}},"sector_count":2,"sectors":[{{"sector":[5,-6,7],"length":3,"sha256":"{digest}"}},{{"sector":[0,0,0],"length":0,"sha256":"{empty}"}}]}}"#,
        quoted(arg(&file))
    );
    assert_eq!(json.trim_end(), expected);
}

/// **A header with nothing optional in it says so**: no engine version and no
/// scene are `null`, and the words a person reads.
#[test]
fn dump_of_a_bare_header_says_what_is_not_recorded() {
    let temp = TempDir::new("dump-bare");
    let header = SaveHeader {
        engine_version: None,
        ..SaveHeader::new(TickId::from_raw(1), 0.0)
    };
    let file = write_save(temp.path(), "bare.crb", header, Vec::new());

    let text = stdout(&crcbl(temp.path(), &["save", "dump", arg(&file)]));
    assert!(text.contains("engine version  not recorded"), "{text}");
    assert!(text.contains("scene           none"), "{text}");
    let json = stdout(&crcbl(temp.path(), &["save", "dump", arg(&file), "--json"]));
    assert!(
        json.contains(r#""engine_version":null,"scene":null,"sector_count":0,"sectors":[]"#),
        "{json}"
    );
}

/// **The version-2 fixture says it was migrated, and from which version**,
/// and the file on disk is left as it was.
#[test]
fn dump_of_a_version_2_save_says_it_was_migrated_in_memory() {
    let temp = TempDir::new("dump-v2");
    let original = std::fs::read(v2_fixture()).expect("the committed version-2 fixture");
    let file = temp.path().join("old.crb");
    std::fs::write(&file, &original).expect("a scratch copy");

    let human = crcbl(temp.path(), &["save", "dump", arg(&file)]);
    assert_eq!(code(&human), OK, "{}", stderr(&human));
    let text = stdout(&human);
    assert!(
        text.contains("format version  2, migrated in memory to 3; the file is unchanged"),
        "{text}"
    );
    assert!(text.contains("tick            9001"), "{text}");
    assert!(text.contains("engine version  not recorded"), "{text}");

    let json = stdout(&crcbl(temp.path(), &["save", "dump", arg(&file), "--json"]));
    assert!(
        json.contains(r#""format_version":2,"current_format_version":3,"migrated":true,"tick":9001,"playtime_secs":3723.25,"engine_version":null,"scene":null,"sector_count":3,"#),
        "{json}"
    );
    assert_eq!(
        std::fs::read(&file).expect("the scratch copy"),
        original,
        "`dump` rewrote the file it read"
    );
}

/// **`--hex` previews a bounded number of bytes** — the CLI's
/// `HEX_PREVIEW_BYTES`, 64 — and says how many more there are.
#[test]
fn dump_hex_shows_a_bounded_preview() {
    const CAP: usize = 64;
    let temp = TempDir::new("dump-hex");
    let long: Vec<u8> = (0..100).collect();
    let file = write_save(
        temp.path(),
        "hex.crb",
        full_header(1),
        vec![sector(0, 0, 0, &long), sector(1, 0, 0, &[0xAB, 0xCD])],
    );

    let text = stdout(&crcbl(temp.path(), &["save", "dump", arg(&file), "--hex"]));
    assert!(
        text.contains("      00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f\n"),
        "{text}"
    );
    assert!(
        text.contains(&format!("      … and {} more bytes", long.len() - CAP)),
        "{text}"
    );
    assert!(text.contains("      ab cd\n"), "{text}");
    assert!(
        !text.contains(" 40 "),
        "a byte past the cap was shown:\n{text}"
    );

    let json = stdout(&crcbl(
        temp.path(),
        &["save", "dump", arg(&file), "--hex", "--json"],
    ));
    let preview: String = long[..CAP]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert!(
        json.contains(&format!(r#""hex":"{preview}","hex_truncated":true}}"#)),
        "{json}"
    );
    assert!(
        json.contains(r#""hex":"abcd","hex_truncated":false}"#),
        "{json}"
    );
    // Without the flag, no preview at all.
    let plain = stdout(&crcbl(temp.path(), &["save", "dump", arg(&file), "--json"]));
    assert!(!plain.contains("\"hex\""), "{plain}");
}

/// **A file the game would refuse is refused by name**, with the game's own
/// text, exit 1 and a machine-readable failure.
#[test]
fn dump_refuses_a_corrupt_or_newer_file_with_the_readers_text() {
    let temp = TempDir::new("dump-refused");
    mixed_directory(temp.path());
    for name in ["newer.crb", "corrupt.crb", "short.crb"] {
        let file = temp.path().join(name);
        let reason = refusal(temp.path(), name);

        let human = crcbl(temp.path(), &["save", "dump", arg(&file)]);
        assert_eq!(code(&human), FAILED, "{name}");
        assert!(stdout(&human).is_empty(), "{name}");
        assert_eq!(
            last_error_line(&human),
            format!("crcbl: cannot open {}: {reason}", file.display()),
            "{name}"
        );

        let json = crcbl(temp.path(), &["save", "dump", arg(&file), "--json"]);
        assert_eq!(code(&json), FAILED, "{name}");
        assert_eq!(
            stdout(&json).trim_end(),
            format!(
                r#"{{"ok":false,"command":"save","error":{},"action":"dump","file":{}}}"#,
                quoted(&format!("cannot open {}: {reason}", file.display())),
                quoted(arg(&file))
            ),
            "{name}"
        );
    }
    assert!(
        refusal(temp.path(), "newer.crb").contains("newer than this build reads"),
        "the newer file is refused as newer"
    );
}

// ── diff ────────────────────────────────────────────────────────────────────

/// **Two saves with the same header and sectors are the same**, exit 0,
/// whatever order each lists its sectors in.
#[test]
fn diff_finds_equal_files_equal() {
    let temp = TempDir::new("diff-same");
    let sectors = || vec![sector(0, 0, 0, &[1, 2]), sector(1, 2, 3, &[3])];
    let a = write_save(temp.path(), "a.crb", full_header(5), sectors());
    let mut reversed = sectors();
    reversed.reverse();
    let b = write_save(temp.path(), "b.crb", full_header(5), reversed);

    let human = crcbl(temp.path(), &["save", "diff", arg(&a), arg(&b)]);
    assert_eq!(code(&human), SAME, "{}", stdout(&human));
    assert!(
        stdout(&human).contains("are the same"),
        "{}",
        stdout(&human)
    );

    let json = crcbl(temp.path(), &["save", "diff", arg(&a), arg(&b), "--json"]);
    assert_eq!(code(&json), SAME);
    assert!(
        stdout(&json).trim_end().ends_with(
            r#""identical":true,"header":[],"only_in_a":[],"only_in_b":[],"changed":[]}"#
        ),
        "{}",
        stdout(&json)
    );
}

/// **A changed sector is found at its first differing offset**, a missing one
/// on the side that has it, and a header field that differs by name — exit 1,
/// with `"ok":true` because the comparison worked.
#[test]
fn diff_finds_a_changed_sector_a_missing_sector_and_a_changed_header() {
    let temp = TempDir::new("diff-differ");
    let a = write_save(
        temp.path(),
        "a.crb",
        full_header(5),
        vec![
            sector(0, 0, 0, &[1, 2, 3, 4, 5]),
            sector(1, 2, 3, &[7, 7]),
            sector(9, 9, 9, &[1]),
        ],
    );
    let b = write_save(
        temp.path(),
        "b.crb",
        full_header(6),
        vec![
            sector(0, 0, 0, &[1, 2, 3, 0, 5, 6]),
            sector(9, 9, 9, &[1]),
            sector(-4, 0, 0, &[]),
        ],
    );

    let human = crcbl(temp.path(), &["save", "diff", arg(&a), arg(&b)]);
    assert_eq!(code(&human), DIFFERENT, "{}", stderr(&human));
    let text = stdout(&human);
    for line in [
        "  header tick: 5 | 6".to_owned(),
        "  sector (0, 0, 0): bytes differ from offset 3; lengths 5 and 6".to_owned(),
        format!("  sector (1, 2, 3): only in {}, 2 bytes", a.display()),
        format!("  sector (-4, 0, 0): only in {}, 0 bytes", b.display()),
    ] {
        assert!(
            text.lines().any(|shown| shown == line),
            "`{line}` missing:\n{text}"
        );
    }
    assert!(
        !text.contains("(9, 9, 9)"),
        "an equal sector was reported:\n{text}"
    );

    let json = crcbl(temp.path(), &["save", "diff", arg(&a), arg(&b), "--json"]);
    assert_eq!(code(&json), DIFFERENT);
    let expected = format!(
        r#"{{"ok":true,"command":"save","action":"diff","a":{},"b":{},"identical":false,"header":[{{"field":"tick","a":5,"b":6}}],"only_in_a":[{{"sector":[1,2,3],"occurrence":1,"length":2}}],"only_in_b":[{{"sector":[-4,0,0],"occurrence":1,"length":0}}],"changed":[{{"sector":[0,0,0],"occurrence":1,"length_a":5,"length_b":6,"first_difference":3}}]}}"#,
        quoted(arg(&a)),
        quoted(arg(&b))
    );
    assert_eq!(stdout(&json).trim_end(), expected);
}

/// **Each header field is compared**: the engine version, the scene, the
/// playtime and the format version, each named when it alone differs.
#[test]
fn diff_names_each_header_field_that_differs() {
    let temp = TempDir::new("diff-header");
    let base = write_save(temp.path(), "base.crb", full_header(1), Vec::new());
    let variants = [
        (
            "engine_version",
            SaveHeader {
                engine_version: None,
                ..full_header(1)
            },
        ),
        (
            "scene",
            SaveHeader {
                scene: None,
                ..full_header(1)
            },
        ),
        (
            "playtime_secs",
            SaveHeader {
                playtime_secs: 4.0,
                ..full_header(1)
            },
        ),
    ];
    for (field, header) in variants {
        let other = write_save(temp.path(), "other.crb", header, Vec::new());
        let output = crcbl(
            temp.path(),
            &["save", "diff", arg(&base), arg(&other), "--json"],
        );
        assert_eq!(code(&output), DIFFERENT, "{field}");
        let json = stdout(&output);
        assert!(
            json.contains(&format!(r#""header":[{{"field":"{field}","#)),
            "{field}: {json}"
        );
    }

    let v2 = temp.path().join("v2.crb");
    std::fs::copy(v2_fixture(), &v2).expect("a scratch copy");
    let data = SaveReader::open(
        &NativeStorage::at(temp.path().to_path_buf()),
        Path::new("v2.crb"),
    )
    .expect("the fixture opens")
    .into_data();
    let mut writer = SaveWriter::new(data.header);
    for sector in data.sectors {
        writer.add_sector(sector);
    }
    let storage = NativeStorage::at(temp.path().to_path_buf());
    writer
        .write(&storage, Path::new("v3.crb"))
        .expect("a scratch save");
    assert!(storage.exists(Path::new("v3.crb")));
    let output = crcbl(
        temp.path(),
        &[
            "save",
            "diff",
            arg(&v2),
            arg(&temp.path().join("v3.crb")),
            "--json",
        ],
    );
    assert_eq!(code(&output), DIFFERENT);
    assert!(
        stdout(&output).contains(
            r#""header":[{"field":"format_version","a":2,"b":3}],"only_in_a":[],"only_in_b":[],"changed":[]"#
        ),
        "{}",
        stdout(&output)
    );
}

/// **A file that does not open is trouble, exit 2**, named with the reader's
/// text, whichever side it is on.
#[test]
fn diff_of_a_file_that_does_not_open_is_trouble() {
    let temp = TempDir::new("diff-trouble");
    mixed_directory(temp.path());
    let good = temp.path().join("good.crb");
    let corrupt = temp.path().join("corrupt.crb");
    let missing = temp.path().join("missing.crb");
    for (a, b, bad) in [
        (&good, &corrupt, &corrupt),
        (&corrupt, &good, &corrupt),
        (&good, &missing, &missing),
    ] {
        let output = crcbl(temp.path(), &["save", "diff", arg(a), arg(b), "--json"]);
        assert_eq!(code(&output), TROUBLE, "{a:?} {b:?}");
        let json = stdout(&output);
        assert!(
            json.starts_with(r#"{"ok":false,"command":"save","error":"#),
            "{json}"
        );
        assert!(
            json.contains(&format!(r#""file":{}"#, quoted(arg(bad)))),
            "{json}"
        );
    }
    let output = crcbl(temp.path(), &["save", "diff", arg(&good), arg(&corrupt)]);
    assert_eq!(
        last_error_line(&output),
        format!(
            "crcbl: cannot open {}: {}",
            corrupt.display(),
            refusal(temp.path(), "corrupt.crb")
        )
    );
}

/// **A malformed invocation is exit 2** on every branch, with nothing on
/// stdout.
#[test]
fn a_malformed_save_invocation_exits_two() {
    let temp = TempDir::new("usage");
    for args in [
        vec!["save"],
        vec!["save", "restore", "a.crb"],
        vec!["save", "dump"],
        vec!["save", "diff", "a.crb"],
        vec!["save", "list", "--hex"],
    ] {
        let output = crcbl(temp.path(), &args);
        assert_eq!(code(&output), 2, "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
    let help = crcbl(temp.path(), &["save", "--help"]);
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).contains("crcbl save"));
}
