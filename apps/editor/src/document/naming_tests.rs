//! Names: a rename and its undo, what a delete, a duplicate and a paste do
//! with a name, and the refusals.

use super::*;

fn document() -> Document {
    Document::built_in().expect("the compiled-in scene is a scene")
}

/// The name `id` has in `document`, as text.
fn name_of(document: &Document, id: SceneEntityId) -> Option<String> {
    document
        .entity_name(id)
        .map(|name| name.as_str().to_owned())
}

/// **A rename is one command whose undo puts the scene back byte for byte** —
/// no names file, no `names` in the header — and whose redo names it again; a
/// second rename replaces the name and clearing the text takes it away.
#[test]
fn a_rename_is_one_command_that_undo_and_redo_walk() {
    let mut document = document();
    let before = document.files().expect("ids");
    let first = SceneEntityId(1);

    assert!(document.rename(first, "  Gate ").expect("a held entity"));
    assert_eq!(name_of(&document, first).as_deref(), Some("Gate"));
    assert_eq!(document.log().len(), 1);
    assert!(document.is_dirty());
    let named = document.files().expect("ids");
    assert_eq!(
        named["names.ron"],
        "Names(\n    names: [\n        (1, \"Gate\"),\n    ],\n)"
    );
    assert!(
        named["scene.ron"].contains("names: true"),
        "{}",
        named["scene.ron"]
    );
    assert_eq!(named["sys/blocks.ron"], before["sys/blocks.ron"]);

    assert!(document.undo().expect("the rename's inverse applies"));
    assert_eq!(document.files().expect("ids"), before);
    assert_eq!(name_of(&document, first), None);
    assert!(!document.is_dirty());
    assert!(document.redo().expect("and again"));
    assert_eq!(document.files().expect("ids"), named);

    assert!(document.rename(first, "Spawner").expect("held"));
    assert_eq!(name_of(&document, first).as_deref(), Some("Spawner"));
    assert!(document.rename(first, "   ").expect("held"));
    assert_eq!(name_of(&document, first), None, "empty text kept the name");
    assert_eq!(document.files().expect("ids"), before);

    assert!(document.undo().expect("the clear's inverse"));
    assert_eq!(name_of(&document, first).as_deref(), Some("Spawner"));
    assert!(document.undo().expect("the second rename's inverse"));
    assert_eq!(name_of(&document, first).as_deref(), Some("Gate"));
}

/// A rename to the name the entity already has records nothing, so it costs
/// no undo.
#[test]
fn a_rename_to_the_same_name_records_nothing() {
    let mut document = document();
    assert!(document.rename(SceneEntityId(2), "Gate").expect("held"));
    assert!(!document.rename(SceneEntityId(2), "Gate ").expect("held"));
    assert!(!document.rename(SceneEntityId(3), "").expect("held"));
    assert_eq!(document.log().len(), 1);
}

/// **A name that is not one is refused by the rule it breaks**, as is a rename
/// of an entity the scene does not hold, and none of them records anything.
#[test]
fn a_rename_that_is_not_a_name_is_refused_and_records_nothing() {
    use crcbl::scene::scn::MAX_NAME_CHARS;

    let mut document = document();
    let before = document.files().expect("ids");
    let long = "n".repeat(MAX_NAME_CHARS + 1);
    let error = document
        .rename(SceneEntityId(1), &long)
        .expect_err("too long");
    assert!(
        matches!(error, EditError::Name(NameError::TooLong { .. })),
        "{error}"
    );
    let error = document
        .rename(SceneEntityId(1), "Ga\tte")
        .expect_err("a tab");
    assert!(
        matches!(error, EditError::Name(NameError::Control('\t'))),
        "{error}"
    );
    let error = document
        .rename(SceneEntityId(9_999), "Gate")
        .expect_err("no such entity");
    assert!(
        matches!(error, EditError::NoEntity(SceneEntityId(9_999))),
        "{error}"
    );
    let error = document
        .apply(EditCommand::Rename {
            entity: SceneEntityId(9_999),
            name: None,
        })
        .expect_err("no such entity");
    assert!(matches!(error, EditError::NoEntity(_)), "{error}");
    assert!(document.log().is_empty());
    assert_eq!(document.files().expect("ids"), before);
}

/// **A rename is refused in play mode**, like every other edit, and the name
/// stays what it was.
#[test]
fn a_rename_is_refused_in_play_mode() {
    let mut document = document();
    document.play().expect("the greybox scene plays");
    // A name that is not one too: the refusal says why it was refused first.
    for text in ["Gate", "Ga\tte"] {
        let error = document
            .rename(SceneEntityId(1), text)
            .expect_err("play mode refuses edits");
        assert!(matches!(error, EditError::Playing), "{text:?}: {error}");
    }
    assert_eq!(name_of(&document, SceneEntityId(1)), None);
    assert!(document.log().is_empty());
}

/// **A deleted entity's name goes with it and comes back with its undo**, so a
/// save in between writes no name for an entity the scene does not hold.
#[test]
fn a_delete_takes_the_name_and_its_undo_brings_it_back() {
    let mut document = document();
    document.rename(SceneEntityId(2), "Gate").expect("held");
    let named = document.files().expect("ids");

    document.delete(&[SceneEntityId(2)]).expect("held");
    let files = document
        .files()
        .expect("a scene with a deleted entity saves");
    assert!(!files.contains_key("names.ron"), "{files:?}");
    assert!(document.undo().expect("the delete's inverse"));
    assert_eq!(document.files().expect("ids"), named);
    assert_eq!(
        name_of(&document, SceneEntityId(2)).as_deref(),
        Some("Gate")
    );
}

/// **A duplicate is unnamed, and a paste keeps a name only while nothing else
/// bears it** — so a copy pasted back into its own scene is unnamed, and one
/// pasted into another scene keeps its name.
#[test]
fn a_duplicate_is_unnamed_and_a_paste_keeps_a_name_nothing_else_bears() {
    let mut document = document();
    document.rename(SceneEntityId(3), "Gate").expect("held");

    let copy = document.duplicate(&[SceneEntityId(3)]).expect("held")[0];
    assert_eq!(
        name_of(&document, copy),
        None,
        "the duplicate took the name"
    );

    let text = document.copy(&[SceneEntityId(3)]).expect("held");
    assert!(text.contains("name: Some(\"Gate\")"), "{text}");
    let pasted = document.paste(&text).expect("a clipping of this scene");
    assert_eq!(name_of(&document, pasted[0]), None, "two entities are Gate");

    let mut other = Document::built_in().expect("the compiled-in scene");
    let pasted = other
        .paste(&text)
        .expect("a clipping of the same vocabulary");
    assert_eq!(name_of(&other, pasted[0]).as_deref(), Some("Gate"));
    assert!(other.undo().expect("the paste's inverse"));
    assert!(
        other.entity_names().is_empty(),
        "the undone paste left a name"
    );

    // Twice in one clipping, into a scene where the name is free: the first
    // takes it and the second does not.
    let entities = crate::clipboard::decode(&text).expect("its own copy");
    let twice = crate::clipboard::encode([entities.clone(), entities].concat());
    let pasted = other.paste(&twice).expect("two of them");
    assert_eq!(name_of(&other, pasted[0]).as_deref(), Some("Gate"));
    assert_eq!(name_of(&other, pasted[1]), None);
}

/// A clipping whose name is not a name is refused, and spawns nothing.
#[test]
fn a_clipping_whose_name_is_not_a_name_is_refused() {
    let mut document = document();
    let text = document.copy(&[SceneEntityId(1)]).expect("held");
    let mut entities = crate::clipboard::decode(&text).expect("its own copy");
    entities[0].name = Some("a\nb".to_owned());
    let error = document
        .paste(&crate::clipboard::encode(entities))
        .expect_err("a line break is not in a name");
    assert!(matches!(error, EditError::Name(_)), "{error}");
    assert_eq!(document.entity_count(), 4);
    assert!(document.log().is_empty());
}

/// **A named scene saved to a directory opens again with its names**, through
/// the scene's own loader.
#[test]
fn a_saved_named_scene_opens_again_with_its_names() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut document = document();
    document.rename(SceneEntityId(0), "Ground").expect("held");
    let expected = document.files().expect("ids");
    document.save_to(dir.path()).expect("a writable directory");

    let mut reopened = Document::open_dir(dir.path(), crate::scene::vocabulary())
        .expect("what we just wrote is a scene");
    assert_eq!(
        name_of(&reopened, SceneEntityId(0)).as_deref(),
        Some("Ground")
    );
    assert_eq!(reopened.files().expect("ids"), expected);
}
