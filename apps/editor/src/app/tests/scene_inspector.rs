//! The inspector while nothing is selected — the scene's own pane — through
//! the loop: a new entity added in a system from it.

use super::*;

use super::files::chord;

/// Towers' path: a system no scene from empty lists, of a component that is
/// no mesh.
const WAYPOINTS: &str = "waypoints";

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
