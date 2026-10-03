//! An outliner row's context menu through the loop: a right press from the
//! shell opens it, and each item carries out what its key does — Delete as
//! one undoable command, Duplicate selecting the copy, Rename putting the
//! text input in the row.

use super::*;

use crcbl::ui::tree::Ui;

/// The outliner row of entity `id` in the built-in scene: one header, then a
/// row per block in id order.
fn row_key(editor: &Editor<HeadlessShell>, id: SceneEntityId) -> NodeKey {
    let index = usize::try_from(id.0).expect("a small id") + 1;
    editor.panels.row_keys()[index]
}

/// Opens `id`'s row menu with a right press and release through the shell, a
/// frame each.
fn right_click(editor: &mut Editor<HeadlessShell>, id: SceneEntityId) {
    let at = centre(editor, row_key(editor, id));
    let window = editor.window;
    let shell = editor.shell_mut();
    shell.move_pointer(window, at, (0.0, 0.0)).expect("live");
    shell
        .button(window, PointerButton::Right, ButtonState::Pressed, Some(at))
        .expect("live");
    editor.frame().expect("a frame");
    editor
        .shell_mut()
        .button(
            window,
            PointerButton::Right,
            ButtonState::Released,
            Some(at),
        )
        .expect("live");
    editor.frame().expect("a frame");
}

/// Clicks the item of `id`'s open row menu whose label starts with `name`.
fn pick(editor: &mut Editor<HeadlessShell>, id: SceneEntityId, name: &str) {
    let ui = editor.panels.ui();
    let item = ui
        .child_keys(Ui::popup_key(row_key(editor, id)))
        .into_iter()
        .find(|&item| {
            ui.child_keys(item)
                .first()
                .and_then(|&label| ui.text(label))
                .is_some_and(|text| text.starts_with(name))
        })
        .unwrap_or_else(|| panic!("{id}'s menu holds no {name}"));
    let at = centre(editor, item);
    click(editor, at);
}

/// **A right-click on a row and Delete deletes that entity, as one command
/// an undo takes back** — and the right press over the outliner turns no
/// camera.
#[test]
fn a_rows_delete_deletes_it_undoably() {
    let mut editor = headless(40);
    editor.frame().expect("a frame");
    let count = editor.document().entity_count();
    let eye = editor.camera.camera();
    let doomed = SceneEntityId(2);

    right_click(&mut editor, doomed);
    assert_eq!(editor.document().selection(), [doomed]);
    pick(&mut editor, doomed, "Delete");
    assert_eq!(editor.document().entity_count(), count - 1);
    assert_eq!(editor.document().log().position(), 1, "not one command");
    assert_eq!(editor.camera.camera(), eye, "the right press orbited");

    editor.act(&Action::Undo);
    assert_eq!(editor.document().entity_count(), count);
    assert!(!editor.document().is_dirty());
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Duplicate selects the copy, and Rename puts the text input in the
/// row**, as Ctrl+D and F2 do.
#[test]
fn a_rows_duplicate_and_rename_go_the_keys_way() {
    let mut editor = headless(40);
    editor.frame().expect("a frame");
    let count = editor.document().entity_count();
    let original = SceneEntityId(1);

    right_click(&mut editor, original);
    pick(&mut editor, original, "Duplicate");
    assert_eq!(editor.document().entity_count(), count + 1);
    let copy = editor.document().primary().expect("the copy is selected");
    assert_ne!(copy, original);

    right_click(&mut editor, original);
    assert_eq!(
        editor.document().selection(),
        [original],
        "a right-click outside the selection did not select the row"
    );
    pick(&mut editor, original, "Rename");
    assert_eq!(editor.panels.renaming(), Some(original));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
