use super::*;

#[derive(Reflect)]
enum Groups {
    Before { first: [f64; 2] },
    After { second: [f64; 2] },
}

#[test]
fn switching_variant_does_not_transfer_open_state_to_another_field() {
    let mut ui = Ui::new();
    let mut value = Groups::Before { first: [1.0, 2.0] };
    page(&mut ui, idle(), NavInput::default(), &mut value, None);
    open(&mut ui, "first", &mut value, None);
    let first = header(&ui, "first");
    assert!(ui.is_open(first));
    assert!(
        !rows(&ui).is_empty(),
        "the first group's body must be built"
    );

    value.set_variant("After").unwrap();
    page(&mut ui, idle(), NavInput::default(), &mut value, None);
    let second = header(&ui, "second");
    assert!(
        !ui.is_open(second),
        "another field inherited the previous field's open state"
    );
    assert_ne!(first, second, "different fields must have different keys");
    assert!(rows(&ui).is_empty(), "the new group must start closed");

    open(&mut ui, "second", &mut value, None);
    assert!(ui.is_open(header(&ui, "second")));
    assert!(!rows(&ui).is_empty());
}
