use super::*;
use crate::tree::Behavior;

fn unchanged(inspection: &Inspection, value: &Surface) {
    assert_eq!(*value, surface());
    assert!(inspection.edits.is_empty());
    assert!(inspection.switches.is_empty());
}

fn toggled(inspection: &Inspection, value: &Surface) {
    let mut expected = surface();
    expected.visible = false;
    assert_eq!(*value, expected);
    assert_eq!(
        inspection.edits,
        [FieldEdit {
            path: "visible".to_owned(),
            before: Value::Bool(true),
            after: Value::Bool(false),
        }]
    );
    assert!(inspection.switches.is_empty());
    assert_eq!(inspection.focused.as_deref(), Some("visible"));
}

#[test]
fn modal_inspector_traps_tree_order_focus_and_restores_its_opener() {
    let mut ui = Ui::new();
    let mut value = surface();
    let step = |ui: &mut Ui, value: &mut Surface, open, nav| {
        frame(ui, idle(), nav, |ui| {
            let earlier = ui.button("#earlier", "Earlier");
            let outside = ui.button("#outside", "Outside");
            let mut inspection = None;
            if open {
                ui.block_with("#modal", &[], Behavior::MODAL, |ui| {
                    inspection = Some(ui.inspector("#props", value));
                });
            }
            assert!(
                !earlier.clicked && !outside.clicked,
                "navigation activated the background"
            );
            assert_ne!(earlier.key, outside.key);
            (outside.key, inspection)
        })
    };
    let (outside, _) = step(&mut ui, &mut value, false, NavInput::default());
    ui.set_focus(outside);
    step(&mut ui, &mut value, false, NavInput::NAVIGATION);
    assert_eq!(ui.focused(), Some(outside));

    // Focus resolution uses the previous tree, including its modal boundary.
    step(&mut ui, &mut value, true, NavInput::NAVIGATION);
    assert_eq!(ui.focused(), Some(outside));
    let (_, inspection) = step(&mut ui, &mut value, true, NavInput::NAVIGATION);
    unchanged(&inspection.unwrap(), &value);
    let order = [
        field_key(&ui, "Label"),
        header(&ui, "Position"),
        header(&ui, "Shape: Dome"),
        header(&ui, "Tint"),
        header(&ui, "Motion"),
        field_key(&ui, "Height"),
        field_key(&ui, "Visible"),
        field_key(&ui, "Layer"),
    ];
    assert_eq!(ui.focused(), Some(order[0]));
    for (nav, expected) in [
        (
            NavInput::NEXT,
            [
                order[1], order[2], order[3], order[4], order[5], order[6], order[7], order[0],
            ],
        ),
        (
            NavInput::PREV,
            [
                order[7], order[6], order[5], order[4], order[3], order[2], order[1], order[0],
            ],
        ),
    ] {
        for key in expected {
            let (_, inspection) = step(&mut ui, &mut value, true, nav);
            assert_eq!(ui.focused(), Some(key));
            unchanged(&inspection.unwrap(), &value);
        }
    }
    ui.set_focus(outside);
    step(&mut ui, &mut value, true, NavInput::NAVIGATION);
    assert_eq!(ui.focused(), Some(order[0]));
    for key in &order[1..=6] {
        step(&mut ui, &mut value, true, NavInput::NEXT);
        assert_eq!(ui.focused(), Some(*key));
    }
    let (_, inspection) = step(&mut ui, &mut value, true, NavInput::ACCEPT);
    toggled(&inspection.unwrap(), &value);
    assert_eq!(ui.focused(), Some(order[6]));

    step(&mut ui, &mut value, false, NavInput::NAVIGATION);
    step(&mut ui, &mut value, false, NavInput::NAVIGATION);
    assert_eq!(ui.focused(), Some(outside));
}

#[test]
fn disabled_inspector_suppresses_pointer_and_navigation_without_replaying() {
    let mut ui = Ui::new();
    let mut value = surface();
    let step = |ui: &mut Ui, value: &mut Surface, pointer, nav, enabled| {
        frame(ui, pointer, nav, |ui| {
            let mut inspection = None;
            ui.enabled(enabled, |ui| {
                inspection = Some(ui.inspector("#props", value));
            });
            inspection.expect("built")
        })
    };
    let none = NavInput::default();
    unchanged(&step(&mut ui, &mut value, idle(), none, true), &value);
    let visible = field_key(&ui, "Visible");
    let on = centre(&ui, visible);
    unchanged(&step(&mut ui, &mut value, press(on), none, true), &value);
    unchanged(&step(&mut ui, &mut value, release(on), none, false), &value);
    unchanged(&step(&mut ui, &mut value, press(on), none, false), &value);
    unchanged(&step(&mut ui, &mut value, release(on), none, false), &value);

    for nav in [NavInput::NEXT, NavInput::PREV, NavInput::ACCEPT] {
        let inspection = step(&mut ui, &mut value, idle(), nav, false);
        unchanged(&inspection, &value);
        assert_eq!(ui.focused(), None);
        assert_eq!(inspection.focused, None);
    }
    unchanged(&step(&mut ui, &mut value, idle(), none, true), &value);
    ui.set_focus(visible);
    let focused = step(&mut ui, &mut value, idle(), NavInput::NAVIGATION, true);
    unchanged(&focused, &value);
    assert_eq!(focused.focused.as_deref(), Some("visible"));
    assert_eq!(ui.focused(), Some(visible));
    unchanged(
        &step(&mut ui, &mut value, idle(), NavInput::ACCEPT, false),
        &value,
    );
    unchanged(&step(&mut ui, &mut value, idle(), none, true), &value);
    unchanged(&step(&mut ui, &mut value, press(on), none, true), &value);
    toggled(&step(&mut ui, &mut value, release(on), none, true), &value);
}
