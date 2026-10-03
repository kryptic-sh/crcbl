//! The inspector while nothing is selected — the scene's own pane — through
//! the loop: a new entity added in a system from it.

use super::*;

use super::files::{chord, type_and_enter};

/// Towers' path: a system no scene from empty lists, of a component that is
/// no mesh.
const WAYPOINTS: &str = "waypoints";

/// The most wheel detents [`scrolled_to`] turns before it gives up: more than
/// the inspector holds rows.
const MAX_DETENTS: usize = 64;

/// Wheels the inspector until the node `find` names, as each frame lays it
/// out, lies inside the inspector's pane — the way a person scrolls to a row
/// under the fold — and hands back its middle.
pub(super) fn scrolled_to(
    editor: &mut Editor<HeadlessShell>,
    find: impl Fn(&Editor<HeadlessShell>) -> NodeKey,
) -> PhysicalPoint {
    let window = editor.window;
    for _ in 0..MAX_DETENTS {
        let ui = editor.panels.ui();
        let pane = editor.panels.props_key().expect("the inspector was built");
        let (top, bottom) = ui.rect(pane).expect("laid out");
        let (min, max) = ui.rect(find(editor)).expect("laid out");
        if min.y >= top.y && max.y <= bottom.y {
            return centre(editor, find(editor));
        }
        // A detent's `+y` scrolls back towards the start.
        let towards_the_end = max.y > bottom.y;
        let over = (top + bottom) * 0.5;
        editor
            .shell_mut()
            .scroll(
                window,
                ScrollDelta::Lines {
                    x: 0.0,
                    y: if towards_the_end { -1.0 } else { 1.0 },
                },
                Some(physical(over)),
            )
            .expect("live");
        editor.frame().expect("a frame");
    }
    panic!("the inspector never scrolled the node into view");
}

/// Clicks `at` twice, the second inside the double-click time of the first:
/// what opens a drag-value for typing.
pub(super) fn double_click(editor: &mut Editor<HeadlessShell>, at: PhysicalPoint) {
    click(editor, at);
    click(editor, at);
}

/// The middle of the add-an-entity button for `system`, as the last frame
/// laid it out.
pub(super) fn entity_add(editor: &Editor<HeadlessShell>, system: &str) -> PhysicalPoint {
    let key = editor
        .panels
        .entity_add_buttons()
        .into_iter()
        .find_map(|(each, key)| (each == system).then_some(key))
        .unwrap_or_else(|| panic!("the inspector offers no new entity in `{system}`"));
    centre(editor, key)
}

/// **With nothing selected, a click on an add-an-entity button puts a new
/// entity in that system and selects it** — the inspector then drawing its
/// section, and offering no new entity while it is selected — and one Ctrl+Z
/// takes the entity and its system's listing back.
#[test]
fn an_add_button_with_nothing_selected_adds_an_entity_and_selects_it() {
    let mut editor = headless(200);
    editor.frame().expect("a frame");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    assert_eq!(editor.document().primary(), None);
    let empty = editor.document_mut().files().expect("ids");

    let at = entity_add(&editor, WAYPOINTS);
    click(&mut editor, at);
    let id = SceneEntityId(0);
    assert_eq!(editor.document_mut().systems_of(id), [WAYPOINTS]);
    assert_eq!(editor.document().primary(), Some(id), "not selected");
    assert_eq!(
        editor.panels.section_systems(),
        [WAYPOINTS],
        "the inspector does not show the new entity",
    );
    assert!(
        editor.panels.entity_add_buttons().is_empty(),
        "a new entity is offered while one is selected",
    );
    assert_eq!(
        editor.panels.status(),
        (format!("Added #{id} in `{WAYPOINTS}`").as_str(), Tone::Info)
    );

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    assert_eq!(editor.document_mut().files().expect("ids"), empty);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// The environment's rows in the inspector: the camera, where it looks, and
/// the ambient light.
const CAMERA_ROW: usize = 0;
/// See [`CAMERA_ROW`].
const AMBIENT_ROW: usize = 2;

/// The middle of the drag-value editing component `axis` of the
/// environment's `row`th row, a vector row — a label, then one cell per
/// component, each a label and then its widget — scrolled into view.
pub(super) fn environment_field(
    editor: &mut Editor<HeadlessShell>,
    row: usize,
    axis: usize,
) -> PhysicalPoint {
    scrolled_to(editor, |editor| {
        let ui = editor.panels.ui();
        let fields = editor
            .panels
            .environment_fields()
            .expect("with nothing selected the inspector draws the environment");
        let cell = ui.child_keys(ui.child_keys(fields)[row])[1 + axis];
        ui.child_keys(cell)[1]
    })
}

/// **With nothing selected the scene's environment is in the inspector, and
/// its fields take the clipboard keys and a drag as a component's do**: a
/// number pasted into the camera's height with Ctrl+V lands there, Ctrl+C
/// copies it back as `env.ron` spells it, and a drag of an ambient channel is
/// one undo.
#[test]
fn the_environment_takes_a_paste_and_a_copy_and_a_drag_is_one_undo() {
    use crcbl::shell::{ClipboardOffer, MimeType};

    let mut editor = headless(200);
    editor.frame().expect("a frame");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    let empty = editor.document_mut().files().expect("ids");
    let window = editor.window;

    let height = environment_field(&mut editor, CAMERA_ROW, 1);
    click(&mut editor, height);
    editor
        .shell_mut()
        .clipboard_offer(window, &[ClipboardOffer::text("32.0")])
        .expect("the headless clipboard takes an offer");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyV);
    for _ in 0..3 {
        editor.frame().expect("a frame");
    }
    assert_eq!(
        editor
            .document()
            .read_environment("camera.1")
            .expect("a leaf"),
        Value::Float(32.0),
        "the paste did not land in the camera's height",
    );
    assert_eq!(editor.document().entity_count(), 0, "the paste spawned");
    editor
        .shell_mut()
        .clipboard_offer(window, &[ClipboardOffer::text("")])
        .expect("the headless clipboard takes an offer");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyC);
    let copied = editor
        .shell
        .clipboard_bytes(MimeType::TextUtf8)
        .expect("the clipboard holds text");
    assert_eq!(copied, b"32.0", "the copy is not the field's text");

    let red = environment_field(&mut editor, AMBIENT_ROW, 0);
    let was = editor
        .document()
        .read_environment("ambient.0")
        .expect("a leaf");
    let entries = editor.document().log().len();
    let from = Vec2::new(red.x as f32, red.y as f32);
    drag(&mut editor, from, from + Vec2::new(30.0, 0.0));
    assert_ne!(
        editor
            .document()
            .read_environment("ambient.0")
            .expect("a leaf"),
        was,
        "the drag did not move the ambient",
    );
    assert_eq!(
        editor.document().log().len(),
        entries + 1,
        "a drag of the ambient is not one entry",
    );
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    assert_eq!(
        editor
            .document()
            .read_environment("ambient.0")
            .expect("a leaf"),
        was
    );
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    assert_eq!(editor.document_mut().files().expect("ids"), empty);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A number typed into a field is one undo**: a double-click opens the
/// camera's height for typing with its number selected, the number typed
/// over it and Enter put it in as one entry in the log, and one Ctrl+Z takes
/// it back. Text the field refuses records nothing.
#[test]
fn a_number_typed_into_a_field_is_one_undo() {
    let mut editor = headless(200);
    editor.frame().expect("a frame");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    let height = |editor: &Editor<HeadlessShell>| {
        editor
            .document()
            .read_environment("camera.1")
            .expect("a leaf")
    };
    let was = height(&editor);
    let entries = editor.document().log().len();

    let at = environment_field(&mut editor, CAMERA_ROW, 1);
    double_click(&mut editor, at);
    // Typed over two frames, so a number put in as it is typed would be an
    // entry a frame rather than the one Enter makes.
    editor.frame().expect("a frame");
    let window = editor.window;
    editor
        .shell_mut()
        .commit_text(window, "17")
        .expect("the headless shell takes text");
    type_and_enter(&mut editor, ".25");
    assert_eq!(
        height(&editor),
        Value::Float(17.25),
        "the number did not go in"
    );
    assert!(
        !editor.panels.text_editing(),
        "Enter did not close the field"
    );
    assert_eq!(
        editor.document().log().len(),
        entries + 1,
        "a typed number is not one entry",
    );

    double_click(&mut editor, at);
    type_and_enter(&mut editor, "seventeen");
    assert!(
        editor.panels.text_editing(),
        "refused text closed the field"
    );
    tap(&mut editor, KeyCode::Escape);
    assert_eq!(height(&editor), Value::Float(17.25));
    assert_eq!(
        editor.document().log().len(),
        entries + 1,
        "refused text recorded an entry",
    );

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    assert_eq!(
        height(&editor),
        was,
        "one undo did not take the number back"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
