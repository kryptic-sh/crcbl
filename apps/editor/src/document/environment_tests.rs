//! The scene's environment edited as a component's fields are: a write, its
//! undo, a drag folded into one entry, a paste, and what each refuses.

use super::*;

use crate::command::Gesture;
use crate::scene::{GREYBOX, built_in_source};

fn document() -> Document {
    Document::built_in().expect("the compiled-in scene is a scene")
}

/// The compiled-in scene's `env.ron`, as committed — what every test here
/// starts from, read off the source rather than retyped.
fn greybox_env() -> String {
    let bytes = built_in_source()
        .read(Path::new(&format!("{GREYBOX}/env.ron")))
        .expect("the compiled-in scene holds its env");
    String::from_utf8(bytes).expect("UTF-8")
}

/// **A write of one leaf of the environment changes that number in
/// `env.ron` and nothing else, and its undo puts the file back byte for
/// byte** — the camera's height, which is `camera.1`, and an ambient
/// channel, written as an `f32` holds it.
#[test]
fn an_environment_write_changes_its_number_in_env_ron_and_undoes() {
    let mut document = document();
    let before = document.files().expect("ids");
    assert_eq!(before["env.ron"], greybox_env());

    document
        .apply(EditCommand::SetEnvironment {
            path: "camera.1".to_owned(),
            value: Value::Float(32.0),
        })
        .expect("a camera has a height");
    document
        .apply(EditCommand::SetEnvironment {
            path: "ambient.0".to_owned(),
            value: Value::Float(0.17),
        })
        .expect("an ambient has a red");
    let after = document.files().expect("ids");
    let expected = greybox_env()
        .replacen(
            "position: (0.0, 6.0, 14.0)",
            "position: (0.0, 32.0, 14.0)",
            1,
        )
        .replacen("ambient: (0.05,", "ambient: (0.17,", 1);
    assert_ne!(
        expected,
        greybox_env(),
        "the greybox env moved; retarget the test"
    );
    assert_eq!(after["env.ron"], expected);
    let mut others = after.clone();
    others.remove("env.ron");
    let mut was = before.clone();
    was.remove("env.ron");
    assert_eq!(
        others, was,
        "a write of the environment touched another file"
    );
    assert!(document.is_dirty());

    assert!(document.undo().expect("applies"));
    assert!(document.undo().expect("applies"));
    assert_eq!(document.files().expect("ids"), before);
    assert!(!document.is_dirty(), "undone to the file, and still dirty");
    assert!(document.redo().expect("applies"));
    assert_eq!(
        document.read_environment("camera.1").expect("a leaf"),
        Value::Float(32.0)
    );
}

/// **A drag's frames are one entry**: the inspector's edits of the
/// environment under one gesture fold into one undo, which goes back to
/// where the drag began.
#[test]
fn an_environment_drag_is_one_undo() {
    let mut document = document();
    let before = document.files().expect("ids");
    let gesture: Gesture = document.begin_gesture();
    let mut was = document.read_environment("look_at.1").expect("a leaf");
    for height in [1.5, 2.0, 2.5] {
        let edit = FieldEdit {
            path: "look_at.1".to_owned(),
            before: was.clone(),
            after: Value::Float(height),
        };
        document
            .record_environment(&[edit], Some(gesture))
            .expect("a drag frame");
        was = Value::Float(height);
    }
    assert_eq!(
        document.log().len(),
        1,
        "a drag of three frames is not one entry"
    );
    assert_eq!(
        document.read_environment("look_at.1").expect("a leaf"),
        Value::Float(2.5)
    );
    assert!(document.undo().expect("applies"));
    assert_eq!(document.files().expect("ids"), before);
}

/// **A pasted value is one command, written as the file spells it, and a
/// copy is the file's own text** — and what a paste or a write refuses
/// changes nothing and records nothing: text of another kind, a number no
/// `f32` holds, a path naming no leaf, and play mode.
#[test]
fn an_environment_paste_writes_the_files_number_and_refusals_change_nothing() {
    let mut document = document();
    assert_eq!(
        document.copy_environment_field("camera.2").expect("a leaf"),
        "14.0"
    );
    document
        .paste_environment_field("ambient.2", "0.22")
        .expect("a number pastes into a channel");
    let pasted = document.files().expect("ids");
    assert!(
        pasted["env.ron"].contains("ambient: (0.05, 0.05, 0.22)"),
        "{}",
        pasted["env.ron"]
    );
    assert_eq!(document.log().len(), 1);

    let error = document
        .paste_environment_field("ambient.2", "dim")
        .expect_err("not a number");
    assert!(matches!(error, EditError::FieldPaste { .. }), "{error}");
    let error = document
        .paste_environment_field("ambient.2", "1e300")
        .expect_err("no f32 holds it");
    assert!(matches!(error, EditError::Path(_)), "{error}");
    let error = document
        .apply(EditCommand::SetEnvironment {
            path: "ambient".to_owned(),
            value: Value::Float(0.5),
        })
        .expect_err("not a leaf");
    assert!(matches!(error, EditError::Path(_)), "{error}");
    document.play().expect("the compiled-in scene plays");
    let error = document
        .paste_environment_field("ambient.2", "0.5")
        .expect_err("playing");
    assert!(matches!(error, EditError::Playing), "{error}");
    document.stop().expect("stops");
    assert_eq!(document.files().expect("ids"), pasted);
    assert_eq!(document.log().len(), 1, "a refusal recorded");
}
