//! The recovery copy, written and read back through the shipped vocabulary.

use super::*;

use crate::document::origin_tests::{props_in_a_game, tree};
use crate::scene::BLOCKS;
use crcbl::scene::scn::SceneEntityId;

/// **A recovery copy is the scene's files, in a new directory named for
/// the moment and the scene**, and the document is left as it was: dirty,
/// with no origin.
#[test]
fn a_recovery_copy_writes_the_scene_and_leaves_the_document_alone() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let mut document = crate::scene::built_in_document().expect("the compiled-in scene");
    document.select(Some(SceneEntityId(0)));
    document
        .apply(crate::command::EditCommand::SetProperty {
            entity: SceneEntityId(0),
            system: BLOCKS.to_owned(),
            path: "position.0".to_owned(),
            value: crcbl::reflect::Value::Float(4.0),
        })
        .expect("a block moves");

    let dir = document
        .write_recovery(base.path(), 1_234)
        .expect("a fresh base");
    assert_eq!(dir, base.path().join("1234-greybox"));
    let files = document.files().expect("ids");
    let written = tree(&dir);
    assert_eq!(written.len(), files.len());
    for (key, text) in &files {
        assert_eq!(
            std::fs::read_to_string(dir.join(key)).expect("written"),
            *text,
            "`{key}` is not the scene's"
        );
    }
    assert!(
        document.is_dirty(),
        "a recovery copy marked the scene saved"
    );
    assert_eq!(document.origin(), None, "the copy became the scene's own");
}

/// **A taken name is passed over, never written into**: a second copy in
/// the same millisecond gets the next name, and what was there is left
/// byte for byte.
#[test]
fn a_recovery_copy_never_overwrites() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let taken = base.path().join("7-greybox");
    std::fs::create_dir(&taken).expect("a fresh base");
    std::fs::write(taken.join("scene.ron"), "a person's own").expect("written");
    let mut document = crate::scene::built_in_document().expect("the compiled-in scene");

    let dir = document
        .write_recovery(base.path(), 7)
        .expect("a free name");
    assert_eq!(dir, base.path().join("7-greybox-1"));
    assert_eq!(
        std::fs::read_to_string(taken.join("scene.ron")).expect("still there"),
        "a person's own"
    );
    let again = document
        .write_recovery(base.path(), 7)
        .expect("a free name");
    assert_eq!(again, base.path().join("7-greybox-2"));
}

/// **A copy made while the scene plays is the scene as authored**, not the
/// played state.
#[test]
fn a_recovery_copy_in_play_is_the_authored_scene() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let mut document = crate::document::play_tests::drifting_document();
    let authored = document.files().expect("ids");
    document.play().expect("the greybox scene plays");
    document.advance(crate::document::play_tests::TICK * 2);
    assert_ne!(
        document.files().expect("ids"),
        authored,
        "nothing moved in play, so this cannot tell the two apart"
    );
    document
        .write_recovery(base.path(), 1)
        .expect("a fresh base");
    let dir = base.path().join("1-greybox");
    for (key, text) in &authored {
        assert_eq!(
            std::fs::read_to_string(dir.join(key)).expect("written"),
            *text
        );
    }
}

/// **A copy is read back unowned and dirty**: the scene it holds, no
/// origin, so a save asks for a directory, and the dirty marker up until
/// a save elsewhere lands — and the copy left as it was.
#[test]
fn a_copy_opens_unowned_and_dirty() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let mut document = crate::document::play_tests::drifting_document();
    let files = document.files().expect("ids");
    let dir = document
        .write_recovery(base.path(), 5)
        .expect("a fresh base");

    let mut opened =
        Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
    assert_eq!(opened.files().expect("ids"), files);
    assert_eq!(opened.origin(), None, "the copy became the scene's home");
    assert!(opened.is_dirty(), "a recovered scene opened clean");
    assert!(matches!(opened.save(), Err(EditError::NoOrigin)));
    let saved = tempfile::tempdir().expect("a temporary directory");
    opened
        .save_as(saved.path().join("kept.scn"))
        .expect("an empty directory");
    assert!(!opened.is_dirty(), "a save-as left it dirty");
    assert_eq!(tree(&dir).len(), files.len(), "the copy was touched");
}

/// **A recovered document remembers its copy until it is taken**, once,
/// and a new scene in its place forgets it; a scene opened from a
/// directory has none.
#[test]
fn a_recovered_document_remembers_its_copy_once() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let mut document = crate::scene::built_in_document().expect("the compiled-in scene");
    assert_eq!(document.take_recovered(), None);
    let dir = document
        .write_recovery(base.path(), 5)
        .expect("a fresh base");

    let mut opened =
        Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
    assert_eq!(opened.take_recovered(), Some(dir.clone()));
    assert_eq!(opened.take_recovered(), None, "taken twice");
    let mut replaced =
        Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
    replaced.new_scene().expect("an empty scene");
    assert_eq!(replaced.take_recovered(), None, "a new scene kept the copy");
}

/// **A copy records where its scene lived, and reads its meshes from
/// there**: the sidecar names the scene's directory and its game's root,
/// the loader passes over it, and the scene read back measures the
/// game's triangle and offers its old directory — with nothing to note.
#[test]
fn a_copy_records_where_its_scene_lived_and_reads_its_meshes_from_there() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (game, scene, mut document) = props_in_a_game();
    let files = document.files().expect("ids");
    let dir = document
        .write_recovery(base.path(), 3)
        .expect("a fresh base");
    assert_eq!(
        std::fs::read_to_string(dir.join(SIDECAR)).expect("a sidecar"),
        format!(
            "origin={}\nassets={}\n",
            scene.display(),
            game.path().display()
        )
    );
    assert_eq!(tree(&dir).len(), files.len() + 1);

    let mut opened =
        Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
    assert_eq!(opened.files().expect("ids"), files, "the sidecar was read");
    assert_eq!(
        opened.mesh_problems().len(),
        1,
        "the recovered scene's meshes are not the game's: {:?}",
        opened.mesh_problems()
    );
    assert_eq!(opened.recorded_origin(), Some(scene.as_path()));
    assert_eq!(opened.origin(), None, "the old directory became its home");
    assert_eq!(opened.take_recovery_notes(), Vec::<String>::new());

    // A copy of the recovered scene still knows where it lived.
    let again = opened.write_recovery(base.path(), 4).expect("a fresh name");
    assert_eq!(
        std::fs::read_to_string(again.join(SIDECAR)).expect("a sidecar"),
        std::fs::read_to_string(dir.join(SIDECAR)).expect("a sidecar"),
    );
}

/// **A copy without a sidecar opens as before**: its meshes read from no
/// asset root and nothing is offered or noted — and a scene that lived
/// nowhere writes none.
#[test]
fn a_copy_without_a_sidecar_opens_as_before() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (_game, _scene, mut document) = props_in_a_game();
    let dir = document
        .write_recovery(base.path(), 3)
        .expect("a fresh base");
    std::fs::remove_file(dir.join(SIDECAR)).expect("a sidecar");

    let mut opened =
        Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
    assert_eq!(opened.mesh_problems().len(), 2, "a mesh was measured");
    assert_eq!(opened.recorded_origin(), None);
    assert_eq!(opened.take_recovery_notes(), Vec::<String>::new());

    let lived_nowhere = crate::scene::built_in_document()
        .expect("the compiled-in scene")
        .write_recovery(base.path(), 5)
        .expect("a fresh name");
    assert!(
        !lived_nowhere.join(SIDECAR).exists(),
        "a sidecar of nothing"
    );
}

/// **A stale sidecar is passed over, never trusted**: a scene directory
/// gone since and an asset root that is now a file are neither offered
/// nor read from, each is noted, and neither is made.
#[test]
fn a_stale_sidecar_is_passed_over_with_notes() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (_game, _scene, mut document) = props_in_a_game();
    let dir = document
        .write_recovery(base.path(), 3)
        .expect("a fresh base");
    let elsewhere = tempfile::tempdir().expect("a temporary directory");
    let gone = elsewhere.path().join("gone.scn");
    let file = elsewhere.path().join("assets");
    std::fs::write(&file, "not a directory").expect("written");
    std::fs::write(
        dir.join(SIDECAR),
        format!("origin={}\nassets={}\n", gone.display(), file.display()),
    )
    .expect("written");

    let mut opened =
        Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
    assert_eq!(opened.recorded_origin(), None, "a gone directory offered");
    assert_eq!(opened.mesh_problems().len(), 2, "read through a file");
    let notes = opened.take_recovery_notes();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes.iter().all(|note| note.contains("passed over")));
    assert!(!gone.exists(), "the gone directory was made");
    assert!(file.is_file(), "the file was touched");
    assert_eq!(opened.take_recovery_notes(), Vec::<String>::new(), "twice");
}
