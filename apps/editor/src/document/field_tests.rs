//! One field on the clipboard: what a copy writes, what a paste applies and
//! undoes, and what a paste refuses.

use super::*;

fn document() -> Document {
    crate::scene::built_in_document().expect("the compiled-in scene is a scene")
}

/// **A field copies as the text the chunk file spells it with**: the third
/// step's `y` and its half width, checked against the committed file's own
/// line rather than against a number this test typed twice.
#[test]
fn a_field_copies_as_the_chunk_files_own_text() {
    let mut document = document();
    let step = SceneEntityId(3);
    let y = document
        .copy_field(step, crate::scene::BLOCKS, "position.1")
        .expect("a block has a y");
    let width = document
        .copy_field(step, crate::scene::BLOCKS, "half_extents.0")
        .expect("and a half width");
    assert_eq!((y.as_str(), width.as_str()), ("1.25", "1.2"));

    let files = document.files().expect("ids");
    let row = files["sys/blocks.ron"]
        .split("(3, Block(")
        .nth(1)
        .expect("the file holds row 3");
    assert!(
        row.contains(&format!("position: (3.0, {y}, 0.0)")),
        "the copy is not the file's text: {row}"
    );

    let error = document
        .copy_field(step, crate::scene::BLOCKS, "position")
        .expect_err("a vector is not a leaf");
    assert!(matches!(error, EditError::Path(_)), "{error}");
}

/// **A pasted value is one command, applied through the component and undone
/// byte for byte** — and a copy of one field pastes into another unchanged.
#[test]
fn a_field_paste_applies_as_one_command_and_undoes() {
    let mut document = document();
    let before = document.files().expect("ids");
    let step = SceneEntityId(3);

    let y = document
        .copy_field(step, crate::scene::BLOCKS, "position.1")
        .expect("a y");
    document
        .paste_field(SceneEntityId(1), crate::scene::BLOCKS, "position.1", &y)
        .expect("a y pastes into a y");
    assert_eq!(
        document
            .read(SceneEntityId(1), crate::scene::BLOCKS, "position.1")
            .expect("a y"),
        Value::Float(1.25),
    );
    document
        .paste_field(step, crate::scene::BLOCKS, "position.0", " -7.5\n")
        .expect("ron reads past surrounding whitespace");
    assert_eq!(
        document
            .read(step, crate::scene::BLOCKS, "position.0")
            .expect("an x"),
        Value::Float(-7.5)
    );
    assert_eq!(document.log().len(), 2, "a paste is one command");
    assert!(document.is_dirty());

    // A value only an `f64` holds copies back exactly as it was pasted.
    let fine = "1.0000000000000002";
    document
        .paste_field(step, crate::scene::BLOCKS, "position.2", fine)
        .expect("a float");
    assert_eq!(
        document
            .copy_field(step, crate::scene::BLOCKS, "position.2")
            .expect("a z"),
        fine
    );

    while document.undo().expect("each inverse applies") {}
    assert_eq!(document.files().expect("ids"), before);
}

/// **A paste that is not the field's kind is refused with a message naming
/// the field, and nothing changes** — text, a value of another kind, a
/// composite, a value the leaf refuses, and a field that is not a leaf.
#[test]
fn a_bad_field_paste_is_refused_and_changes_nothing() {
    let mut document = document();
    let before = document.files().expect("ids");
    let step = SceneEntityId(3);
    for text in ["up a bit", "true", "\"1.0\"", "(1.0, 2.0, 3.0)", ""] {
        let error = document
            .paste_field(step, crate::scene::BLOCKS, "position.1", text)
            .expect_err("not a float");
        assert!(
            matches!(&error, EditError::FieldPaste { path, .. } if path == "position.1"),
            "{text:?}: {error}"
        );
        assert!(error.to_string().contains("position.1"), "{error}");
    }
    let error = document
        .paste_field(step, crate::scene::BLOCKS, "position.1", "inf")
        .expect_err("a position is finite");
    assert!(matches!(error, EditError::Path(_)), "{error}");
    let error = document
        .paste_field(step, crate::scene::BLOCKS, "position", "(1.0, 2.0, 3.0)")
        .expect_err("not a leaf");
    assert!(matches!(error, EditError::Path(_)), "{error}");

    assert!(document.log().is_empty(), "a refused paste was recorded");
    assert_eq!(document.files().expect("ids"), before);
}

/// A field paste is refused in play mode, **before the text is read** — so a
/// paste of text that is no value says why it was refused rather than what
/// was wrong with the clipboard.
#[test]
fn a_field_paste_is_refused_in_play_mode() {
    let mut document = document();
    document.play().expect("the greybox scene plays");
    for text in ["2.0", "up a bit"] {
        let error = document
            .paste_field(SceneEntityId(3), crate::scene::BLOCKS, "position.1", text)
            .expect_err("play mode refuses edits");
        assert!(matches!(error, EditError::Playing), "{text:?}: {error}");
    }
    assert!(document.log().is_empty());
}
