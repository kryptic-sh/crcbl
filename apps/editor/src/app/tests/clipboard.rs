//! The clipboard keys routed by where the keyboard is — one inspector field,
//! or the selected entity — and F2 through the loop.

use super::*;

use crcbl::shell::{ClipboardOffer, MimeType};

/// The pointer moved to `at`, and a frame for the panels to see it there.
fn hover(editor: &mut Editor<HeadlessShell>, at: PhysicalPoint) {
    let window = editor.window;
    editor
        .shell_mut()
        .move_pointer(window, at, (0.0, 0.0))
        .expect("live");
    editor.frame().expect("a frame");
}

/// The middle of the drag-value editing component `axis` of the first row,
/// `position`, of the inspector's first section: a vector row is a label and
/// then one axis cell per component, each a label and then its widget.
fn position_field(editor: &Editor<HeadlessShell>, axis: usize) -> PhysicalPoint {
    let ui = editor.panels.ui();
    let fields = editor
        .panels
        .section_fields(0)
        .expect("the inspector drew a section");
    let row = ui.child_keys(fields)[0];
    let cell = ui.child_keys(row)[1 + axis];
    centre(editor, ui.child_keys(cell)[1])
}

/// The middle of the viewport pane, which is no field.
fn viewport_middle(editor: &Editor<HeadlessShell>) -> PhysicalPoint {
    let (min, max) = editor.panels.viewport_pixels();
    let at = ((min + max) * 0.5).floor();
    PhysicalPoint {
        x: f64::from(at.x),
        y: f64::from(at.y),
    }
}

/// What the headless clipboard holds as text.
fn clipboard_text(editor: &Editor<HeadlessShell>) -> String {
    let bytes = editor
        .shell
        .clipboard_bytes(MimeType::TextUtf8)
        .expect("the clipboard holds text");
    String::from_utf8(bytes.to_vec()).expect("UTF-8")
}

/// Enough frames for the headless clipboard to answer a read.
fn settle(editor: &mut Editor<HeadlessShell>) {
    for _ in 0..3 {
        editor.frame().expect("a frame");
    }
}

/// **With the pointer over a field, Ctrl+C copies that field and Ctrl+V pastes
/// into the field under it, as one undo; with it over the scene, the same keys
/// copy and paste the selected entity.**
#[test]
fn the_clipboard_keys_act_on_the_field_under_the_pointer_or_else_the_entity() {
    let mut editor = headless(200);
    let step = SceneEntityId(3);
    editor.document_mut().select(Some(step));
    editor.frame().expect("a frame");
    let before = editor.document_mut().files().expect("ids");
    let count = editor.document().entity_count();

    let y = position_field(&editor, 1);
    hover(&mut editor, y);
    editor.act(&Action::Copy);
    assert_eq!(
        clipboard_text(&editor),
        "1.25",
        "the field's y was not copied"
    );

    let x = position_field(&editor, 0);
    hover(&mut editor, x);
    editor.act(&Action::Paste);
    settle(&mut editor);
    assert_eq!(
        editor
            .document_mut()
            .read(step, crate::scene::BLOCKS, "position.0")
            .expect("an x"),
        Value::Float(1.25),
        "the field under the pointer did not take the paste",
    );
    assert_eq!(
        editor.document().entity_count(),
        count,
        "a field paste spawned"
    );
    assert_eq!(editor.document().log().len(), 1);
    editor.act(&Action::Undo);
    assert_eq!(editor.document_mut().files().expect("ids"), before);

    let middle = viewport_middle(&editor);
    hover(&mut editor, middle);
    editor.act(&Action::Copy);
    assert!(
        clipboard_text(&editor).starts_with("Entities("),
        "with no field under the pointer the copy was not the entity's: {}",
        clipboard_text(&editor),
    );
    editor.act(&Action::Paste);
    settle(&mut editor);
    assert_eq!(
        editor.document().entity_count(),
        count + 1,
        "the entity paste did not spawn",
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A paste lands where the keyboard was when the key went down**, though
/// the pointer leaves the field before the clipboard answers — and a paste of
/// text that is no value of the field's kind is on the status line, and
/// changes nothing.
#[test]
fn a_field_paste_lands_where_it_was_asked_and_a_bad_one_changes_nothing() {
    let mut editor = headless(200);
    let step = SceneEntityId(3);
    editor.document_mut().select(Some(step));
    editor.frame().expect("a frame");
    let window = editor.window;

    editor
        .shell_mut()
        .clipboard_offer(window, &[ClipboardOffer::text("2.5")])
        .expect("the headless clipboard takes an offer");
    let x = position_field(&editor, 0);
    hover(&mut editor, x);
    editor.act(&Action::Paste);
    let middle = viewport_middle(&editor);
    hover(&mut editor, middle);
    settle(&mut editor);
    assert_eq!(
        editor
            .document_mut()
            .read(step, crate::scene::BLOCKS, "position.0")
            .expect("an x"),
        Value::Float(2.5),
    );
    let pasted = editor.document_mut().files().expect("ids");

    editor
        .shell_mut()
        .clipboard_offer(window, &[ClipboardOffer::text("up a bit")])
        .expect("the headless clipboard takes an offer");
    hover(&mut editor, x);
    editor.act(&Action::Paste);
    settle(&mut editor);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains("no value for `position.0`"), "{text}");
    assert_eq!(editor.document_mut().files().expect("ids"), pasted);
    assert_eq!(
        editor.document().log().len(),
        1,
        "the bad paste was recorded"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **F2 renames the selection in its row, and says why when it cannot**:
/// nothing selected, or play mode.
#[test]
fn f2_starts_a_rename_of_the_selection_and_says_why_when_it_cannot() {
    let mut editor = headless(200);
    editor.frame().expect("a frame");
    tap(&mut editor, KeyCode::F2);
    assert_eq!(editor.panels.renaming(), None);
    assert_eq!(editor.panels.status().0, RENAME_NOTHING);

    editor.document_mut().select(Some(SceneEntityId(1)));
    editor.act(&Action::PlayStop);
    tap(&mut editor, KeyCode::F2);
    assert_eq!(editor.panels.renaming(), None);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains("play mode"), "{text}");

    editor.act(&Action::PlayStop);
    tap(&mut editor, KeyCode::F2);
    assert_eq!(editor.panels.renaming(), Some(SceneEntityId(1)));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// The middle of the widget of the `row`th row of the inspector's `section`th
/// section, a leaf row: a label and then its widget.
fn leaf_field(editor: &Editor<HeadlessShell>, section: usize, row: usize) -> PhysicalPoint {
    let ui = editor.panels.ui();
    let fields = editor
        .panels
        .section_fields(section)
        .expect("the inspector drew the section");
    let row = ui.child_keys(fields)[row];
    centre(editor, ui.child_keys(row)[1])
}

/// **A mass of zero pasted into a body's field is refused on the status
/// line, naming the field, and the body keeps its mass** — the inspector's
/// field half held to the body's rule like every other write.
#[test]
fn a_massless_paste_into_a_body_is_refused_on_the_status_line() {
    use crcbl::scene_physics::BODIES;

    /// A body's rows: its kind, then its mass.
    const MASS_ROW: usize = 1;

    let mut editor = headless(200);
    let step = SceneEntityId(3);
    editor
        .document_mut()
        .attach(step, BODIES)
        .expect("the step has no body");
    editor.document_mut().select(Some(step));
    editor.frame().expect("a frame");
    let before = editor.document_mut().files().expect("ids");
    let window = editor.window;

    editor
        .shell_mut()
        .clipboard_offer(window, &[ClipboardOffer::text("0.0")])
        .expect("the headless clipboard takes an offer");
    let mass = leaf_field(&editor, 1, MASS_ROW);
    hover(&mut editor, mass);
    editor.act(&Action::Paste);
    settle(&mut editor);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(
        text.contains("`mass` in `bodies` is refused"),
        "the status line does not name the field: {text}"
    );
    assert_eq!(
        editor
            .document_mut()
            .read(step, BODIES, "mass")
            .expect("a mass"),
        Value::Float(1.0),
    );
    assert_eq!(editor.document_mut().files().expect("ids"), before);
    assert_eq!(editor.document().log().len(), 1, "the paste was recorded");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
