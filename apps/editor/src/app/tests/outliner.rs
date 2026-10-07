use super::*;

#[test]
fn shift_arrows_extend_and_shrink_the_outliner_selection() {
    let mut editor = headless(40);
    editor.frame().expect("a frame");
    let row = context_menu::row_key(&editor, SceneEntityId(1));
    let at = centre(&editor, row);
    click(&mut editor, at);
    assert_eq!(editor.document().selection(), [SceneEntityId(1)]);

    files::chord(&mut editor, Modifiers::SHIFT, KeyCode::ArrowDown);
    assert_eq!(editor.document().selection(), [1, 2].map(SceneEntityId));
    assert_eq!(editor.document().primary(), Some(SceneEntityId(2)));
    files::chord(&mut editor, Modifiers::SHIFT, KeyCode::ArrowDown);
    assert_eq!(editor.document().selection(), [1, 2, 3].map(SceneEntityId));
    assert_eq!(editor.document().primary(), Some(SceneEntityId(3)));
    files::chord(&mut editor, Modifiers::SHIFT, KeyCode::ArrowUp);
    assert_eq!(editor.document().selection(), [1, 2].map(SceneEntityId));
    assert_eq!(editor.document().primary(), Some(SceneEntityId(2)));

    tap(&mut editor, KeyCode::ArrowUp);
    assert_eq!(editor.document().selection(), [1, 2].map(SceneEntityId));
    assert_eq!(
        editor.document().log().len(),
        0,
        "navigation edited the scene"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
