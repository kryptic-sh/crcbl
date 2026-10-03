//! A new scene, and save-as making its directory the document's origin: later
//! saves land there, an occupied directory is refused, nothing in the old one
//! is touched, and the asset root follows unless it was named.

use super::*;

use crcbl_scene::gltf_fixture::{BIN_CHUNK_BUFFER, glb, triangle_bin, triangle_json};

use super::mesh_tests::TRIANGLE;
use super::systems_tests::{SUN, two_systems};

/// The sun chunk's key in [`two_systems`].
const SUN_CHUNK: &str = "sys/sun.ron";

/// The first step of [`two_systems`], a block and a sun.
const BOTH: SceneEntityId = SceneEntityId(1);

/// A sun alone in [`two_systems`].
const LONE_SUN: SceneEntityId = SceneEntityId(5);

/// Every file under `dir`, keyed by its path from `dir` with `/` between
/// the parts, and its bytes — what a test compares a directory by to say
/// nothing in it changed. Empty for a directory that is not there.
pub(crate) fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                walk(root, &path, files);
                continue;
            }
            let key = path
                .strip_prefix(root)
                .expect("under the root")
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.insert(key, std::fs::read(&path).expect("a readable file"));
        }
    }
    let mut files = BTreeMap::new();
    walk(dir, dir, &mut files);
    files
}

/// `files` as [`tree`] reads them back off a disk.
fn as_bytes(files: &BTreeMap<String, String>) -> BTreeMap<String, Vec<u8>> {
    files
        .iter()
        .map(|(key, text)| (key.clone(), text.as_bytes().to_vec()))
        .collect()
}

/// A game's folder: its manifest, which marks it as the asset root, and the
/// fixture triangle under [`TRIANGLE`].
pub(crate) fn game() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").expect("writable");
    let triangle = dir.path().join(TRIANGLE);
    std::fs::create_dir_all(triangle.parent().expect("a key in a folder")).expect("writable");
    std::fs::write(
        &triangle,
        glb(&triangle_json(BIN_CHUNK_BUFFER), Some(&triangle_bin())),
    )
    .expect("writable");
    dir
}

/// **A new scene holds nothing, lists no system, has no origin and is
/// clean**, with nothing to undo — from a document that had all of them —
/// and keeps the asset source it had.
#[test]
fn a_new_scene_holds_nothing_and_has_no_origin() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    two_systems()
        .save_to(dir.path())
        .expect("a fresh directory");
    let mut document =
        Document::open_dir(dir.path(), crate::scene::vocabulary()).expect("what was written");
    document.set_assets(Box::new(super::mesh_tests::assets()));
    document.select(Some(BOTH));
    document.rename(BOTH, "First step").expect("held");
    let membership = document.membership();
    let naming = document.naming();

    document.new_scene().expect("editing");

    assert_eq!(document.entity_count(), 0);
    assert!(document.outline().is_empty(), "a system is listed");
    assert_eq!(
        document
            .files()
            .expect("nothing to give an id")
            .into_keys()
            .collect::<Vec<_>>(),
        ["env.ron", "scene.ron"],
    );
    assert_eq!(document.name(), crate::scene::UNTITLED);
    assert_eq!(document.origin(), None);
    assert!(!document.is_dirty(), "a new scene opens dirty");
    assert!(document.log().is_empty(), "the old history came along");
    assert!(document.selection().is_empty());
    assert_ne!(
        document.membership(),
        membership,
        "a view would not re-read"
    );
    assert_ne!(document.naming(), naming, "a view would keep the old names");
    assert!(
        document.measure(TRIANGLE).is_ok(),
        "the asset source was not kept"
    );
    assert!(
        matches!(document.save(), Err(EditError::NoOrigin)),
        "a new scene saved over the directory the old one came from",
    );
}

/// **A new scene is refused in play mode**, and the playing scene goes on as
/// it was.
#[test]
fn a_new_scene_is_refused_in_play_mode() {
    let mut document = Document::built_in().expect("the compiled-in scene");
    let count = document.entity_count();
    document.play().expect("the greybox scene plays");
    assert!(matches!(document.new_scene(), Err(EditError::Playing)));
    assert_eq!(document.play_state(), PlayState::Playing);
    assert_eq!(document.entity_count(), count);
}

/// **Save-as makes the directory the document's origin**: a scene that had
/// none saves there again — which a copy refused the second time — and an
/// unlisted system's chunk is removed from it by a later save, because the
/// save-as's files are what it owns now.
#[test]
fn save_as_makes_the_directory_the_origin_and_later_saves_go_there() {
    let mut document = two_systems();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let target = dir.path().join("two.scn");

    document.save_as(&target).expect("a new directory");
    assert_eq!(document.origin(), Some(target.as_path()));
    assert!(!document.is_dirty());
    assert_eq!(tree(&target), as_bytes(&document.files().expect("ids")));

    // The first save after the save-as: a later one owns what an earlier
    // save there wrote whatever save-as did, so only this one can tell.
    document
        .detach(BOTH, SUN)
        .expect("the step keeps its block");
    document.delete(&[LONE_SUN]).expect("held");
    document
        .apply(EditCommand::UnlistSystem {
            system: SUN.to_owned(),
        })
        .expect("the sun is empty");
    document.save().expect("its own directory now");
    assert!(
        !target.join(SUN_CHUNK).exists(),
        "save-as did not hand the document its files"
    );
    assert_eq!(tree(&target), as_bytes(&document.files().expect("ids")));

    document.rename(BOTH, "First step").expect("held");
    document.save().expect("its own directory");
    assert_eq!(tree(&target), as_bytes(&document.files().expect("ids")));
}

/// **Save-as leaves the old directory alone**: later saves go to the new one
/// and nothing in the old one is written or removed, though the scene
/// stopped naming a chunk there.
#[test]
fn save_as_leaves_the_old_directory_untouched() {
    let old = tempfile::tempdir().expect("a temporary directory");
    two_systems()
        .save_to(old.path())
        .expect("a fresh directory");
    let opened = tree(old.path());
    let mut document =
        Document::open_dir(old.path(), crate::scene::vocabulary()).expect("what was written");
    let new = tempfile::tempdir().expect("a temporary directory");

    document.save_as(new.path()).expect("an empty directory");
    document
        .detach(BOTH, SUN)
        .expect("the step keeps its block");
    document.delete(&[LONE_SUN]).expect("held");
    document
        .apply(EditCommand::UnlistSystem {
            system: SUN.to_owned(),
        })
        .expect("the sun is empty");
    document.save().expect("the new origin");

    assert_eq!(tree(old.path()), opened, "the old directory changed");
    assert_eq!(tree(new.path()), as_bytes(&document.files().expect("ids")));
}

/// **Save-as into a directory holding another scene is refused**, writing
/// nothing and moving nothing: the origin, the dirty marker and both
/// directories stay as they were.
#[test]
fn save_as_into_an_occupied_directory_is_refused() {
    let occupied = tempfile::tempdir().expect("a temporary directory");
    Document::built_in()
        .expect("the compiled-in scene")
        .save_to(occupied.path())
        .expect("a fresh directory");
    let there = tree(occupied.path());

    let mut document = two_systems();
    document.rename(BOTH, "First step").expect("held");
    let error = document
        .save_as(occupied.path())
        .expect_err("another scene is there");
    assert!(
        matches!(&error, EditError::Occupied { key, .. } if key == "env.ron"),
        "{error}"
    );
    assert_eq!(
        document.origin(),
        None,
        "a refused save-as moved the origin"
    );
    assert!(document.is_dirty(), "a refused save-as cleared the marker");
    assert_eq!(
        tree(occupied.path()),
        there,
        "it wrote into the other scene"
    );
    assert!(matches!(document.save(), Err(EditError::NoOrigin)));
}

/// **Save-as into the document's own origin is a save**, removing what the
/// scene stopped owning there, and moves no asset source.
#[test]
fn save_as_into_its_own_origin_is_a_save() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    two_systems()
        .save_to(dir.path())
        .expect("a fresh directory");
    let mut document =
        Document::open_dir(dir.path(), crate::scene::vocabulary()).expect("what was written");
    document
        .detach(BOTH, SUN)
        .expect("the step keeps its block");
    document.delete(&[LONE_SUN]).expect("held");
    document
        .apply(EditCommand::UnlistSystem {
            system: SUN.to_owned(),
        })
        .expect("the sun is empty");
    let moved = document.save_as(dir.path()).expect("its own directory");
    assert!(!moved, "saving in place moved the asset source");
    assert!(!dir.path().join(SUN_CHUNK).exists(), "the chunk stayed");
}

/// **The asset root follows save-as to the new directory's game**, measured
/// afresh, when the document's source was not named — and a named source
/// stays where it was named.
#[test]
fn the_asset_root_follows_save_as_unless_it_was_named() {
    let game = game();
    let scene = game.path().join("levels").join("one.scn");

    let mut document = Document::built_in().expect("the compiled-in scene");
    assert!(
        document.measure(TRIANGLE).is_err(),
        "the fixture is visible already"
    );
    let moved = document.save_as(&scene).expect("a new directory");
    assert!(moved, "the asset source did not move");
    assert!(
        document.measure(TRIANGLE).is_ok(),
        "the source is not the new directory's game"
    );

    let mut named = Document::built_in().expect("the compiled-in scene");
    named.set_assets(Box::new(MemorySource::new()));
    let moved = named
        .save_as(game.path().join("levels").join("two.scn"))
        .expect("a new directory");
    assert!(!moved, "a named source moved");
    assert!(
        named.measure(TRIANGLE).is_err(),
        "a named source was replaced"
    );
}

/// **A typed target is checked before anything is written**: nothing typed,
/// a control character and a file are each refused, and a relative path is
/// made absolute with its surrounding whitespace dropped.
#[test]
fn a_typed_target_is_checked_at_the_boundary() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "plans").expect("writable");
    for text in [
        "",
        "   ",
        "levels/\u{7}one.scn",
        &file.display().to_string(),
    ] {
        assert!(
            matches!(save_target(text), Err(EditError::Target { .. })),
            "{text:?} was taken"
        );
    }
    let target = save_target("  levels/one.scn ").expect("a relative directory");
    assert!(target.is_absolute(), "{}", target.display());
    assert!(target.ends_with(Path::new("levels").join("one.scn")));
}
