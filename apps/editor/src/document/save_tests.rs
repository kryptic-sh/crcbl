//! What a save removes from a scene directory, and what it leaves: the chunks
//! the scene stopped owning, and nothing else — see `document::ownership`.

use super::*;

use super::systems_tests::{SUN, two_systems};

/// The sun chunk's key, which [`on_disk`]'s manifest lists.
const SUN_CHUNK: &str = "sys/sun.ron";

/// The names chunk's key.
const NAMES_CHUNK: &str = "names.ron";

/// The first step, a block and a sun.
const BOTH: SceneEntityId = SceneEntityId(1);

/// A sun alone.
const LONE_SUN: SceneEntityId = SceneEntityId(5);

/// [`two_systems`] written to a fresh directory and opened from it, so the
/// document owns that directory's files and saves back over them.
fn on_disk() -> (tempfile::TempDir, Document) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    two_systems()
        .save_to(dir.path())
        .expect("a fresh directory takes the copy");
    let document = reopen(dir.path());
    (dir, document)
}

/// The scene at `dir`, opened through the vocabulary [`two_systems`] uses.
fn reopen(dir: &Path) -> Document {
    Document::open_dir(dir, crate::scene::vocabulary()).expect("the directory holds a scene")
}

/// Empties the sun — the step leaves it, the lone sun is deleted — and takes
/// it out of the manifest: three entries, the last the unlisting.
fn unlist_sun(document: &mut Document) {
    document
        .detach(BOTH, SUN)
        .expect("the step keeps its block");
    document.delete(&[LONE_SUN]).expect("the lone sun is held");
    document
        .apply(EditCommand::UnlistSystem {
            system: SUN.to_owned(),
        })
        .expect("the sun holds no entity now");
}

/// **A save after a system is unlisted removes its chunk**, and the directory
/// opens again as the scene that was saved — with `sys/` holding the one
/// chunk the manifest names.
#[test]
fn an_unlisted_systems_chunk_is_removed_and_the_scene_reopens_the_same() {
    let (dir, mut document) = on_disk();
    assert!(dir.path().join(SUN_CHUNK).is_file());
    unlist_sun(&mut document);
    let expected = document.files().expect("ids");
    assert!(!expected.contains_key(SUN_CHUNK));

    document.save().expect("its own directory is writable");
    assert!(!dir.path().join(SUN_CHUNK).exists(), "the sun chunk stayed");
    assert!(!document.is_dirty());
    let chunks: Vec<_> = std::fs::read_dir(dir.path().join("sys"))
        .expect("the save wrote `sys/`")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    assert_eq!(chunks, ["blocks.ron"]);
    assert_eq!(reopen(dir.path()).files().expect("ids"), expected);
}

/// **Clearing the last name removes `names.ron`**, from a document that read
/// the names chunk when it opened rather than wrote it in this session.
#[test]
fn clearing_the_last_name_removes_the_names_chunk() {
    let (dir, mut document) = on_disk();
    document.rename(BOTH, "First step").expect("held");
    document.save().expect("its own directory is writable");
    assert!(dir.path().join(NAMES_CHUNK).is_file());

    let mut document = reopen(dir.path());
    document.rename(BOTH, "").expect("held");
    let expected = document.files().expect("ids");
    assert!(!expected.contains_key(NAMES_CHUNK));
    document.save().expect("its own directory is writable");
    assert!(
        !dir.path().join(NAMES_CHUNK).exists(),
        "the names chunk stayed"
    );
    assert_eq!(reopen(dir.path()).files().expect("ids"), expected);
}

/// **A save removes nothing the scene never wrote**: a note beside the
/// scene, and a chunk under `sys/` no manifest of this document named, are
/// both there after the save that removes the sun, byte for byte.
#[test]
fn a_save_leaves_every_file_the_scene_did_not_write() {
    let (dir, mut document) = on_disk();
    let note = dir.path().join("notes.txt");
    let stray = dir.path().join("sys").join("stray.ron");
    std::fs::write(&note, "the ramp is too steep").expect("a writable directory");
    std::fs::write(&stray, "Chunk(system: \"stray\", entities: [])").expect("a writable directory");

    unlist_sun(&mut document);
    document.save().expect("its own directory is writable");
    assert!(!dir.path().join(SUN_CHUNK).exists(), "the sun chunk stayed");
    assert_eq!(
        std::fs::read_to_string(&note).expect("the note survives"),
        "the ramp is too steep"
    );
    assert_eq!(
        std::fs::read_to_string(&stray).expect("the stray chunk survives"),
        "Chunk(system: \"stray\", entities: [])"
    );
}

/// **A directory where the save owned a chunk file is left alone**: it was
/// put there since, it is not the file the document wrote, and removing it
/// would take whatever is inside with it.
#[test]
fn a_directory_where_an_owned_chunk_was_is_left_alone() {
    let (dir, mut document) = on_disk();
    let sun = dir.path().join(SUN_CHUNK);
    std::fs::remove_file(&sun).expect("the save wrote the sun chunk");
    std::fs::create_dir(&sun).expect("a writable directory");
    let inside = sun.join("keep.txt");
    std::fs::write(&inside, "mine").expect("a writable directory");

    unlist_sun(&mut document);
    // A hand edit of the document's own files is a change on disk, which a
    // save refuses to overwrite until it is accepted; what is held here is
    // what the save that does overwrite does.
    document.accept_changes_on_disk();
    document.save().expect("its own directory is writable");
    assert_eq!(
        std::fs::read_to_string(&inside).expect("the directory survives"),
        "mine"
    );
}

/// **A save that fails part way removes nothing** — the sun chunk the old
/// manifest named is still there beside the blocks chunk that would not
/// write — and the document stays dirty. Once the obstacle is gone the next
/// save removes it, because the document still owns it.
///
/// The failure is real rather than injected: a directory holding a file
/// where `sys/blocks.ron` goes, which the atomic write's rename cannot
/// replace on any platform. It lands after `scene.ron`, so the manifest on
/// disk has already stopped naming the sun when the save fails.
#[test]
fn a_failed_save_keeps_the_old_chunks_and_the_next_save_removes_them() {
    let (dir, mut document) = on_disk();
    let sun_before = std::fs::read(dir.path().join(SUN_CHUNK)).expect("the sun chunk");
    unlist_sun(&mut document);

    let blocks = dir.path().join("sys").join("blocks.ron");
    std::fs::remove_file(&blocks).expect("the save wrote the blocks chunk");
    std::fs::create_dir(&blocks).expect("a writable directory");
    let obstacle = blocks.join("obstacle.txt");
    std::fs::write(&obstacle, "in the way").expect("a writable directory");

    // A hand edit of the document's own files is a change on disk, which a
    // save refuses to overwrite until it is accepted; what is held here is
    // what the save that does overwrite does.
    document.accept_changes_on_disk();
    let error = document.save().expect_err("a directory is in the way");
    assert!(
        matches!(&error, EditError::Write { key, .. } if key == "sys/blocks.ron"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(dir.path().join(SUN_CHUNK)).expect("the sun chunk survives"),
        sun_before
    );
    assert!(document.is_dirty());

    std::fs::remove_file(&obstacle).expect("the obstacle is there");
    std::fs::remove_dir(&blocks).expect("and now empty");
    document.save().expect("nothing is in the way now");
    assert!(!dir.path().join(SUN_CHUNK).exists(), "the sun chunk stayed");
    assert!(!document.is_dirty());
}

/// **A chunk a failed save did write is still the document's**: the names
/// chunk lands before the sun chunk's write fails, and once the name is
/// cleared the next save removes it — without a reopen in between to read it
/// from the manifest.
#[test]
fn a_chunk_a_failed_save_wrote_is_still_owned() {
    let (dir, mut document) = on_disk();
    document.rename(BOTH, "First step").expect("held");

    let sun = dir.path().join(SUN_CHUNK);
    std::fs::remove_file(&sun).expect("the save wrote the sun chunk");
    std::fs::create_dir(&sun).expect("a writable directory");
    let obstacle = sun.join("obstacle.txt");
    std::fs::write(&obstacle, "in the way").expect("a writable directory");
    // A hand edit of the document's own files is a change on disk, which a
    // save refuses to overwrite until it is accepted; what is held here is
    // what the save that does overwrite does.
    document.accept_changes_on_disk();
    let error = document.save().expect_err("a directory is in the way");
    assert!(
        matches!(&error, EditError::Write { key, .. } if key == SUN_CHUNK),
        "{error}"
    );
    assert!(dir.path().join(NAMES_CHUNK).is_file(), "it landed first");

    std::fs::remove_file(&obstacle).expect("the obstacle is there");
    std::fs::remove_dir(&sun).expect("and now empty");
    document.rename(BOTH, "").expect("held");
    document.save().expect("nothing is in the way now");
    assert!(
        !dir.path().join(NAMES_CHUNK).exists(),
        "the names chunk stayed"
    );
}

/// **A chunk already gone is removed**: a person deleting the sun chunk by
/// hand before the save that would have does not fail that save.
#[test]
fn a_chunk_already_gone_does_not_fail_the_save() {
    let (dir, mut document) = on_disk();
    unlist_sun(&mut document);
    std::fs::remove_file(dir.path().join(SUN_CHUNK)).expect("the save wrote it");
    // A hand edit of the document's own files is a change on disk, which a
    // save refuses to overwrite until it is accepted; what is held here is
    // what the save that does overwrite does.
    document.accept_changes_on_disk();
    document.save().expect("nothing is left to remove");
    assert!(!document.is_dirty());
}

/// **A save into another directory is a copy**: it removes nothing from the
/// document's own, writes only the scene as it stands, and refuses — writing
/// nothing — once that directory holds a file it would write. A directory
/// holding only files the scene never writes takes it beside them.
#[test]
fn a_save_elsewhere_removes_nothing_and_overwrites_nothing() {
    let (dir, mut document) = on_disk();
    unlist_sun(&mut document);

    let copy = tempfile::tempdir().expect("a temporary directory");
    document
        .save_to(copy.path())
        .expect("an empty directory takes it");
    assert!(dir.path().join(SUN_CHUNK).is_file(), "the copy removed one");
    assert!(!copy.path().join(SUN_CHUNK).exists());
    assert!(copy.path().join("sys").join("blocks.ron").is_file());

    let header = std::fs::read(copy.path().join("scene.ron")).expect("the copy's header");
    document.rename(BOTH, "First step").expect("held");
    let error = document
        .save_to(copy.path())
        .expect_err("the copy is not the document's own");
    assert!(
        matches!(&error, EditError::Occupied { key, .. } if key == "env.ron"),
        "{error}"
    );
    assert!(
        !copy.path().join(NAMES_CHUNK).exists(),
        "it wrote something"
    );
    assert_eq!(
        std::fs::read(copy.path().join("scene.ron")).expect("the header"),
        header
    );

    let beside = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(beside.path().join("notes.txt"), "plans").expect("writable");
    document
        .save_to(beside.path())
        .expect("nothing there is a file the scene writes");
    assert!(beside.path().join("scene.ron").is_file());
    assert!(beside.path().join("notes.txt").is_file());
}

/// **The document's own directory spelled another way is still its own**, so
/// a save naming it removes what the scene stopped owning there.
#[test]
fn the_documents_own_directory_spelled_another_way_is_its_own() {
    let (dir, mut document) = on_disk();
    unlist_sun(&mut document);
    let respelled = dir.path().join("sys").join("..");
    assert_ne!(respelled.as_path(), dir.path());
    document
        .save_to(&respelled)
        .expect("its own directory is writable");
    assert!(!dir.path().join(SUN_CHUNK).exists(), "the sun chunk stayed");
}

/// **Undoing an unlisting and saving writes the chunk again**, and undoing
/// every edit and saving puts the directory back as it was opened.
#[test]
fn undoing_an_unlist_and_saving_writes_the_chunk_again() {
    let (dir, mut document) = on_disk();
    let opened = document.files().expect("ids");
    unlist_sun(&mut document);
    document.save().expect("its own directory is writable");
    assert!(!dir.path().join(SUN_CHUNK).exists());

    assert!(document.undo().expect("the unlisting undoes"));
    document.save().expect("its own directory is writable");
    let listed = document.files().expect("ids");
    assert!(listed.contains_key(SUN_CHUNK));
    assert!(
        dir.path().join(SUN_CHUNK).is_file(),
        "the sun chunk is back"
    );
    assert_eq!(reopen(dir.path()).files().expect("ids"), listed);

    // The chunk is this session's own writing now, not the opened manifest's,
    // and a redo of the unlisting removes it again.
    assert!(document.redo().expect("the unlisting redoes"));
    document.save().expect("its own directory is writable");
    assert!(!dir.path().join(SUN_CHUNK).exists(), "the sun chunk stayed");
    assert!(document.undo().expect("the unlisting undoes"));

    while document.undo().expect("each edit undoes") {}
    document.save().expect("its own directory is writable");
    assert_eq!(document.files().expect("ids"), opened);
    assert_eq!(
        std::fs::read_to_string(dir.path().join(SUN_CHUNK)).expect("the sun chunk"),
        opened[SUN_CHUNK]
    );
    assert_eq!(reopen(dir.path()).files().expect("ids"), opened);
}
