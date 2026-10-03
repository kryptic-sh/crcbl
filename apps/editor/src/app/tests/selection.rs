//! Several entities selected, through the loop: clicks in the viewport, the
//! shared-pivot translate, the modes that refuse several, and the actions
//! that take the whole selection.

use super::*;

/// The window pixel the middle of `id`'s box is drawn at — asserted to be on
/// no gizmo handle, so a click there is a pick.
fn pixel_of(editor: &mut Editor<HeadlessShell>, id: SceneEntityId) -> PhysicalPoint {
    let (min, max) = editor.document_mut().bounds(id).expect("a placed entity");
    let (corner, _) = editor.panels.viewport_pixels();
    let at = editor
        .camera
        .camera()
        .pixel_of((min + max) * 0.5, editor.panels.viewport_extent())
        .expect("the default view has it in front of the eye");
    let handles = editor.handles();
    assert!(
        gizmo::hit(&handles, at, editor.panels.scale()).is_none(),
        "{id}'s middle is under a handle, so a click there would grab it",
    );
    physical(corner + at)
}

/// Holds Ctrl down, or lets it go, a frame for the loop to see it.
fn hold_ctrl(editor: &mut Editor<HeadlessShell>, held: bool) {
    let window = editor.window;
    let shell = editor.shell_mut();
    if held {
        shell.set_modifiers(Modifiers::CTRL);
        shell.key_press(window, KeyCode::ControlLeft).expect("live");
    } else {
        shell.set_modifiers(Modifiers::empty());
        shell
            .key_release(window, KeyCode::ControlLeft)
            .expect("live");
    }
    editor.frame().expect("a frame");
}

/// **A click in the viewport selects what it lands on alone; with Ctrl held
/// it adds that entity as the primary or takes it out, and a Ctrl click on
/// nothing keeps the selection.**
#[test]
fn a_viewport_click_replaces_and_a_ctrl_click_toggles() {
    let mut editor = headless(80);
    let [left, right] = [SceneEntityId(1), SceneEntityId(3)];
    // Scale, whose handles several selected do not show and one shows about
    // its own box: translate's would stand at the pivot, between the two,
    // over the very entities clicked.
    tap(&mut editor, KeyCode::KeyR);

    let at = pixel_of(&mut editor, right);
    click(&mut editor, at);
    assert_eq!(editor.document().selection(), [right]);

    hold_ctrl(&mut editor, true);
    let at = pixel_of(&mut editor, left);
    click(&mut editor, at);
    assert_eq!(editor.document().selection(), [right, left]);
    assert_eq!(editor.document().primary(), Some(left));

    // Above everything: the scene's top is well under the pane's top edge.
    let (corner, _) = editor.panels.viewport_pixels();
    click(&mut editor, physical(corner + Vec2::splat(4.0)));
    assert_eq!(
        editor.document().selection(),
        [right, left],
        "a Ctrl click on nothing dropped the selection",
    );

    let at = pixel_of(&mut editor, right);
    click(&mut editor, at);
    assert_eq!(editor.document().selection(), [left], "Ctrl did not toggle");

    hold_ctrl(&mut editor, false);
    let at = pixel_of(&mut editor, right);
    click(&mut editor, at);
    assert_eq!(
        editor.document().selection(),
        [right],
        "a click did not replace"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// The middle of `id`'s box, in simulation space.
fn centre_of(editor: &mut Editor<HeadlessShell>, id: SceneEntityId) -> DVec3 {
    editor
        .document_mut()
        .placement(id)
        .expect("a placed entity")
        .centre
}

/// **With several selected, the translate handles stand at the selection's
/// pivot, and dragging one moves every selected entity by the same delta
/// along that axis alone — as one undo.**
#[test]
fn a_shared_pivot_drag_moves_every_selected_entity_as_one_undo() {
    let mut editor = headless(24);
    let picked = [SceneEntityId(1), SceneEntityId(3)];
    editor.document_mut().set_selection(picked);
    editor.frame().expect("a frame");
    let before = editor.document_mut().files().expect("ids");
    let was = picked.map(|id| leaves(&mut editor, id, gizmo::POSITION));

    let pivot = editor.document_mut().selection_pivot().expect("placed");
    let (centre, _) = editor.gizmo_centre().expect("translate shows handles");
    assert_eq!(centre, pivot.as_vec3(), "the handles are not at the pivot");

    let (from, to) = handle_at(&mut editor, gizmo::Grip::Move(gizmo::Axis::X));
    drag(&mut editor, (from + to) * 0.5, to + (to - from) * 0.5);

    assert_eq!(editor.document().selection(), picked, "the press re-picked");
    let now = picked.map(|id| leaves(&mut editor, id, gizmo::POSITION));
    let moved = now[0][0] - was[0][0];
    assert!(
        moved > 0.1,
        "the X handle did not move them along +X: {now:?}"
    );
    assert!(
        (now[1][0] - was[1][0] - moved).abs() < 1e-9,
        "they moved by different deltas: {was:?} to {now:?}",
    );
    for (now, was) in now.iter().zip(&was) {
        assert_eq!([now[1], now[2]], [was[1], was[2]], "one moved off its axis");
    }
    assert_eq!(editor.document().log().len(), 1, "a drag is one entry");

    editor.act(&Action::Undo);
    assert_eq!(editor.document_mut().files().expect("ids"), before);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Snapping a shared-pivot drag lands the pivot on the absolute grid** and
/// moves every entity by the same delta, from a start the arrow keys left off
/// it — so neither snapping each entity nor stepping from the press passes.
#[test]
fn a_snapped_shared_pivot_drag_lands_the_pivot_on_the_grid() {
    let mut editor = headless(32);
    let picked = [SceneEntityId(1), SceneEntityId(3)];
    editor.document_mut().set_selection([picked[1]]);
    editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
    editor.document_mut().set_selection(picked);
    editor.frame().expect("a frame");
    let step = editor.snap.grid_step();
    let off = editor.document_mut().selection_pivot().expect("placed").x;
    assert!(
        (off / step - (off / step).round()).abs() > 1e-6,
        "the pivot is on the grid already, so this proves nothing: {off}",
    );
    let was = picked.map(|id| centre_of(&mut editor, id));

    hold_ctrl(&mut editor, true);
    let (from, to) = handle_at(&mut editor, gizmo::Grip::Move(gizmo::Axis::X));
    drag(&mut editor, (from + to) * 0.5, to + (to - from) * 0.5);

    let now = editor.document_mut().selection_pivot().expect("placed").x;
    assert!(
        now > off + step,
        "the drag did not move them: {off} to {now}"
    );
    assert!(
        (now / step - (now / step).round()).abs() < 1e-9,
        "the pivot {now} is not on the {step} m grid",
    );
    let moved: Vec<f64> = picked
        .iter()
        .zip(&was)
        .map(|(id, was)| centre_of(&mut editor, *id).x - was.x)
        .collect();
    assert!(
        (moved[0] - moved[1]).abs() < 1e-9,
        "the snap moved them by different deltas: {moved:?}",
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Scale and rotate refuse several selected**: R and E show no handles and
/// say why on the status line, and W brings the shared translate back.
#[test]
fn scale_and_rotate_show_no_handles_with_several_selected() {
    let mut editor = headless(40);
    editor
        .document_mut()
        .set_selection([SceneEntityId(1), SceneEntityId(3)]);
    editor.frame().expect("a frame");
    for (key, mode) in [
        (KeyCode::KeyR, gizmo::Mode::Scale),
        (KeyCode::KeyE, gizmo::Mode::Rotate),
    ] {
        tap(&mut editor, key);
        assert_eq!(editor.gizmo_mode, mode);
        assert!(editor.handles().is_empty(), "{mode:?} shows handles");
        let (text, tone) = editor.panels.status();
        assert_eq!(tone, Tone::Warning, "{text}");
        assert!(text.contains("2 entities are selected"), "{text}");
    }
    tap(&mut editor, KeyCode::KeyW);
    assert!(
        !editor.handles().is_empty(),
        "W did not bring translate back"
    );

    editor.document_mut().select(Some(SceneEntityId(3)));
    tap(&mut editor, KeyCode::KeyR);
    assert!(
        !editor.handles().is_empty(),
        "one selected shows no scale handles"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Delete and duplicate take the whole selection, one undo each, and the
/// copies are selected in place of their originals**; an arrow nudges every
/// selected entity, as one entry too.
#[test]
fn delete_duplicate_and_nudge_take_the_whole_selection() {
    let mut editor = headless(8);
    let picked = [SceneEntityId(1), SceneEntityId(3)];
    let count = editor.document().entity_count();
    let before = editor.document_mut().files().expect("ids");

    editor.document_mut().set_selection(picked);
    editor.act(&Action::Delete);
    assert_eq!(editor.document().entity_count(), count - 2);
    assert!(editor.document().selection().is_empty());
    assert_eq!(editor.document().log().len(), 1, "a delete is one entry");
    editor.act(&Action::Undo);
    assert_eq!(editor.document_mut().files().expect("ids"), before);

    editor.document_mut().set_selection(picked);
    editor.act(&Action::Duplicate);
    assert_eq!(editor.document().entity_count(), count + 2);
    let copies = editor.document().selection().to_vec();
    assert_eq!(copies.len(), 2, "the copies are not what is selected");
    assert!(copies.iter().all(|copy| !picked.contains(copy)));
    assert_eq!(
        editor.document().log().position(),
        1,
        "a duplicate is one entry"
    );
    editor.act(&Action::Undo);
    assert_eq!(editor.document_mut().files().expect("ids"), before);

    editor.document_mut().set_selection(picked);
    let was = picked.map(|id| leaves(&mut editor, id, gizmo::POSITION)[0]);
    editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
    let now = picked.map(|id| leaves(&mut editor, id, gizmo::POSITION)[0]);
    assert_eq!(now, was.map(|x| x + NUDGE_M), "the nudge missed one");
    assert_eq!(
        editor.document().log().position(),
        1,
        "a nudge is one entry"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Every selected entity is outlined, the primary in its own colour.**
#[test]
fn every_selected_entity_is_outlined_and_the_primary_distinctly() {
    let mut document = crate::scene::built_in_document().expect("the compiled-in scene");
    document.set_selection([SceneEntityId(1), SceneEntityId(3)]);
    let boxes = selection_boxes(&mut document);
    let colors: Vec<[f32; 4]> = boxes.iter().map(|(_, color)| *color).collect();
    assert_eq!(colors, [SELECTED_COLOR, PRIMARY_COLOR]);
    assert_ne!(SELECTED_COLOR, PRIMARY_COLOR);
    let primary = document.placement(SceneEntityId(3)).expect("placed");
    assert_eq!(boxes[1].0, primary.corners().map(|corner| corner.as_vec3()));
}
