//! The whole scene read back from the document's own directory through the
//! reload's difference — `scene_edit::reload`'s module docs, _Revert is every
//! chunk reloaded_, hold the decisions.

use super::reload_tests::{BLOCKS_RON, PADS, opened, two_systems, write_blocks, x_of};
use super::tests::BLOCKS;
use super::*;

/// The edits the document makes here before it reverts: a block and a pad
/// moved, a block renamed and the ambient light changed — a chunk row, a
/// name and the environment, each of which a revert reads back.
fn edit_everything(document: &mut Document) {
    for (system, entity, x) in [(BLOCKS, 0, 7.0), (PADS, 2, 27.0)] {
        document
            .apply(EditCommand::SetProperty {
                entity: SceneEntityId(entity),
                system: system.to_owned(),
                path: "position.0".to_owned(),
                value: Value::Float(x),
            })
            .expect("moves");
    }
    assert!(document.rename(SceneEntityId(1), "Post").expect("a name"));
    document
        .apply(EditCommand::SetEnvironment {
            path: "ambient.0".to_owned(),
            value: Value::Float(0.5),
        })
        .expect("an ambient light");
}

/// **A revert makes the document hold what the disk holds, as one entry,
/// and leaves it clean**: chunks, names and the environment, with another
/// program's change to a chunk taken in; one undo brings every edit back,
/// and the document is dirty again.
#[test]
fn a_revert_reads_the_disk_back_as_one_entry_that_undo_takes_back() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = opened(&base);
    edit_everything(&mut document);
    let edited = document.files().expect("ids");
    let entries = document.log().len();
    write_blocks(
        &dir,
        &BLOCKS_RON.replace("(1.0, 0.0, 0.0)", "(4.0, 0.0, 0.0)"),
    );
    let disk = Document::open_dir(&dir, two_systems())
        .expect("the disk reads")
        .files()
        .expect("ids");

    assert!(document.revert().expect("reverts"));
    assert_eq!(
        document.files().expect("ids"),
        disk,
        "the document is not the disk's"
    );
    assert_eq!(document.log().len(), entries + 1, "a revert is one entry");
    assert!(!document.is_dirty(), "a reverted document is dirty");
    document
        .save()
        .expect("the save does not refuse over what the revert read");

    assert!(document.undo().expect("the revert walks back"));
    assert_eq!(
        document.files().expect("ids"),
        edited,
        "undo did not bring the edits back"
    );
    assert!(document.is_dirty());
}

/// **A revert of a document the disk already holds changes nothing** and
/// records nothing.
#[test]
fn a_revert_with_nothing_to_take_back_records_nothing() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (_dir, mut document) = opened(&base);
    assert!(!document.revert().expect("reverts"));
    assert!(document.log().is_empty());
    assert!(!document.is_dirty());
}

/// **A header changed on disk is not reverted**: the revert refuses with
/// [`EditError::HeaderChanged`] and changes nothing, so the caller reads the
/// scene again whole.
#[test]
fn a_header_changed_on_disk_refuses_a_revert() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = opened(&base);
    edit_everything(&mut document);
    let edited = document.files().expect("ids");
    let header = dir.join("scene.ron");
    let text = std::fs::read_to_string(&header).expect("a header");
    std::fs::write(&header, text.replace("\"two\"", "\"renamed\"")).expect("written");

    match document.revert() {
        Err(EditError::HeaderChanged(at)) => assert_eq!(at, dir),
        other => panic!("a changed header was reverted: {other:?}"),
    }
    assert_eq!(document.files().expect("ids"), edited);
    assert_eq!(x_of(&mut document, 0, BLOCKS), Some(Value::Float(7.0)));
}
