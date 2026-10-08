use super::*;

#[test]
fn changing_space_finishes_the_current_drag_without_losing_undo() {
    let mut editor = headless(24);
    editor.document_mut().select(Some(SceneEntityId(2)));
    editor.frame().unwrap();
    let before = editor.document_mut().files().unwrap();
    let (from, to) = handle_at(&mut editor, gizmo::Grip::Move(gizmo::Axis::X));
    let direction = to - from;
    let window = editor.window;
    editor
        .shell_mut()
        .button(
            window,
            PointerButton::Left,
            ButtonState::Pressed,
            Some(physical(from + direction * 0.8)),
        )
        .unwrap();
    editor.frame().unwrap();
    editor
        .shell_mut()
        .move_pointer(window, physical(from + direction * 1.3), (0.0, 0.0))
        .unwrap();
    editor.frame().unwrap();
    assert!(matches!(editor.drag, Some(Drag::Gizmo(..))));
    let landed = editor.document_mut().files().unwrap();
    assert_ne!(landed, before);
    editor.act(&Action::ToggleSpace);
    assert!(editor.drag.is_none());
    editor
        .shell_mut()
        .move_pointer(window, physical(from + direction * 1.8), (0.0, 0.0))
        .unwrap();
    editor.frame().unwrap();
    assert_eq!(editor.document_mut().files().unwrap(), landed);
    editor.act(&Action::Undo);
    assert_eq!(editor.document_mut().files().unwrap(), before);
    editor.finish(ExitReason::FrameBudget).unwrap();
}

#[test]
fn the_space_key_changes_translate_axes_and_preserves_scale_axes() {
    let mut editor = headless(40);
    let id = SceneEntityId(2);
    let rotation = DQuat::from_rotation_z(0.4) * DQuat::from_rotation_y(0.7);
    crate::document::rotation_tests::turn_block(editor.document_mut(), id, rotation);
    editor.document_mut().select(Some(id));
    editor.frame().unwrap();
    assert_eq!(editor.gizmo_space, gizmo::Space::World);
    let world = handle_at(&mut editor, gizmo::Grip::Move(gizmo::Axis::X));
    tap(&mut editor, KeyCode::KeyX);
    assert_eq!(editor.gizmo_space, gizmo::Space::Local);
    let local = handle_at(&mut editor, gizmo::Grip::Move(gizmo::Axis::X));
    assert_ne!(world, local);
    assert!(editor.panels.status().0.contains("Local axes"));

    let before = editor.document_mut().files().unwrap();
    let start = DVec3::from_array(leaves(&mut editor, id, gizmo::POSITION));
    let entries = editor.document().log().len();
    let direction = local.1 - local.0;
    drag(
        &mut editor,
        local.0 + direction * 0.8,
        local.0 + direction * 1.4,
    );
    let moved = DVec3::from_array(leaves(&mut editor, id, gizmo::POSITION)) - start;
    let axis = rotation * DVec3::X;
    assert!(
        moved.dot(axis) > 0.01,
        "the drag did not move along local X: {moved}"
    );
    assert!(
        moved.cross(axis).length() < 1e-5,
        "the drag moved off local X: {moved}"
    );
    assert_eq!(editor.document().log().len(), entries + 1);
    editor.act(&Action::Undo);
    assert_eq!(editor.document_mut().files().unwrap(), before);

    tap(&mut editor, KeyCode::KeyR);
    let local_scale = editor.handles();
    tap(&mut editor, KeyCode::KeyX);
    assert_eq!(editor.gizmo_space, gizmo::Space::World);
    assert_eq!(editor.handles(), local_scale);
    editor.finish(ExitReason::FrameBudget).unwrap();
}
