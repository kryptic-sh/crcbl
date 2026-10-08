//! A rotation in the inspector: three angles over the quaternion the file
//! holds, and a dragged angle one command.

use super::*;

use crcbl::math::DQuat;
use crcbl::reflect::Value;

/// The row a `Block`'s rotation is drawn on: after its position and its half
/// extents.
const ROTATION_ROW: usize = 2;

/// The Y angle on that row.
const Y: usize = 1;

/// How far each test drags an angle, in pixels.
const PIXELS: f32 = 40.0;

/// `id`'s block rotation, read off its four leaves.
fn rotation(page: &mut Page, id: SceneEntityId) -> DQuat {
    let leaf = |page: &mut Page, name: &str| match page.document.read(
        id,
        crate::scene::BLOCKS,
        &format!("rotation.{name}"),
    ) {
        Ok(Value::Float(value)) => value,
        other => panic!("rotation.{name} is {other:?}"),
    };
    DQuat::from_xyzw(
        leaf(page, "x"),
        leaf(page, "y"),
        leaf(page, "z"),
        leaf(page, "w"),
    )
}

/// **Dragging a rotation's angle turns the block about that axis, as one
/// command over every frame of the drag**, its quaternion still unit — and
/// one undo puts all four leaves back at once.
#[test]
fn dragging_a_rotation_angle_is_one_command_that_turns_the_block() {
    let mut page = Page::built_in();
    let id = SceneEntityId(3);
    page.document.select(Some(id));
    page.idle();
    let before = page.document.files().expect("the scene saves");

    let field = page.axis_field(ROTATION_ROW, Y);
    let at = page.centre(field);
    page.drag_in_steps(at, Vec2::new(PIXELS, 0.0), 4);

    assert_eq!(
        page.document.log().len(),
        1,
        "a drag of an angle is one entry"
    );
    let turned = rotation(&mut page, id);
    assert!(
        (turned.length() - 1.0).abs() < 1e-12,
        "{turned:?} is not unit"
    );
    let degrees = crate::panel::inspector::degrees_of(turned);
    assert!(
        degrees[0].abs() < 1e-9 && degrees[2].abs() < 1e-9,
        "dragging Y turned it about another axis: {degrees:?}",
    );
    assert!(degrees[1] > 1.0, "the drag did not turn it: {degrees:?}");

    assert!(page.document.undo().expect("one entry"));
    assert_eq!(page.document.files().expect("the scene saves"), before);
}

#[test]
fn dragging_pitch_through_a_right_angle_keeps_the_requested_orientation() {
    let mut page = Page::built_in();
    let id = SceneEntityId(3);
    let initial = crate::panel::inspector::quat_of([20.0, 80.0, 30.0]);
    crate::document::rotation_tests::turn_block(&mut page.document, id, initial);
    page.document.select(Some(id));
    page.idle();
    let before = page.document.files().unwrap();
    let entries = page.document.log().len();
    let at = page.centre(page.axis_field(ROTATION_ROW, Y));
    page.drag_in_steps(at, Vec2::new(80.0, 0.0), 8);

    let expected = crate::panel::inspector::quat_of([20.0, 120.0, 30.0]);
    let actual = rotation(&mut page, id);
    assert!(
        actual.abs_diff_eq(expected, 1e-12) || actual.abs_diff_eq(-expected, 1e-12),
        "pitch drag changed the other axes: {actual:?}, expected {expected:?}"
    );
    assert_eq!(page.document.log().len(), entries + 1);
    assert!(page.document.undo().unwrap());
    assert_eq!(page.document.files().unwrap(), before);
}

#[test]
fn an_external_rotation_change_invalidates_the_dragged_rows_angles() {
    let mut page = Page::built_in();
    let id = SceneEntityId(3);
    crate::document::rotation_tests::turn_block(
        &mut page.document,
        id,
        crate::panel::inspector::quat_of([20.0, 80.0, 30.0]),
    );
    page.document.select(Some(id));
    page.idle();
    let at = page.centre(page.axis_field(ROTATION_ROW, Y));
    let held = |pos| PointerInput {
        pos,
        down: true,
        released: false,
        secondary_pressed: false,
    };
    page.frame(held(at), 0.0);
    page.frame(held(at + Vec2::new(30.0, 0.0)), 0.0);
    crate::document::rotation_tests::turn_block(
        &mut page.document,
        id,
        crate::panel::inspector::quat_of([60.0, 20.0, 10.0]),
    );
    page.frame(held(at + Vec2::new(80.0, 0.0)), 0.0);

    // The dragged number still measures from its press; the other axes must
    // come from the replacement value rather than the retained representation.
    let expected = crate::panel::inspector::quat_of([60.0, 120.0, 10.0]);
    let actual = rotation(&mut page, id);
    assert!(
        actual.abs_diff_eq(expected, 1e-12) || actual.abs_diff_eq(-expected, 1e-12),
        "external rotation lost: {actual:?}, expected {expected:?}"
    );
}

/// **The angles and the quaternion are one orientation both ways**, away
/// from the right-angle pitch where Euler angles trade places.
#[test]
fn the_rows_angles_compose_back_to_the_quaternion() {
    for degrees in [[0.0, 0.0, 0.0], [30.0, -45.0, 60.0], [-120.0, 10.0, 170.0]] {
        let quat = crate::panel::inspector::quat_of(degrees);
        let back = crate::panel::inspector::degrees_of(quat);
        assert!(
            crate::panel::inspector::quat_of(back).abs_diff_eq(quat, 1e-12),
            "{degrees:?} read back as {back:?}",
        );
        for (shown, set) in back.iter().zip(degrees) {
            assert!(
                (shown - set).abs() < 1e-9,
                "{degrees:?} read back as {back:?}"
            );
        }
    }
}
