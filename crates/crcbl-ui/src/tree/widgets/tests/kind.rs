//! What each widget builder records its nodes as: the roles the accessibility
//! tree gives them come from this, not from the selector.

use super::*;
use crate::tree::TextInputOptions;
use crate::tree::widgets::WidgetKind;

/// What `key` was recorded as this frame.
fn kind_of(ui: &Ui, key: NodeKey) -> WidgetKind {
    ui.store.by_key(key).expect("stored").kind
}

/// **Each builder records what it built, whatever type the caller's selector
/// names** — a caller-named type replaces the widget's own in the selector,
/// and the record stays — and a plain block records nothing.
#[test]
fn each_builder_records_what_it_built_whatever_type_the_selector_names() {
    let mut ui = Ui::new();
    let mut checked = true;
    let mut volume = 0.5_f32;
    let mut count = 3.0_f64;
    let mut name = String::new();
    let mut secret = String::from("hunter2");
    let mut chosen = 0;
    let built = frame(&mut ui, idle(), NavInput::default(), |ui| {
        let masked = TextInputOptions {
            masked: true,
            ..TextInputOptions::default()
        };
        [
            (ui.button("fancy#ok", "OK").key, WidgetKind::Button),
            (
                ui.checkbox("#agree", "Agree", &mut checked).key,
                WidgetKind::Checkbox,
            ),
            (
                ui.slider("#volume", &mut volume, 0.0..=1.0, 0.1).key,
                WidgetKind::Slider,
            ),
            (
                ui.drag_value("#count", &mut count, 0.0..=10.0, 1.0, 1.0)
                    .key,
                WidgetKind::DragValue,
            ),
            (
                ui.text_input("#name", &mut name).key,
                WidgetKind::TextInput { masked: false },
            ),
            (
                ui.text_input_with("#secret", &mut secret, masked).key,
                WidgetKind::TextInput { masked: true },
            ),
            (
                ui.select("#size", &["S", "M"], &mut chosen).key,
                WidgetKind::Select,
            ),
            (
                ui.collapsing("#more", "More", |_| {}).key,
                WidgetKind::Disclosure,
            ),
            (ui.tree_leaf("#leaf", "Leaf").key, WidgetKind::TreeRow),
            (ui.list("#rows", 2, 10.0, |_, _| {}).key, WidgetKind::List),
            (ui.block("#plain", &[], |_| {}).key, WidgetKind::None),
        ]
    });
    for (key, kind) in built {
        assert_eq!(kind_of(&ui, key), kind, "{key:?} recorded the wrong kind");
    }

    let agree = built[1].0;
    let check = children_of(&ui, agree)[0];
    assert_eq!(
        kind_of(&ui, check),
        WidgetKind::Decoration,
        "a checkbox's box is not marked as drawn for looks alone"
    );
    let list = built[9].0;
    let content = children_of(&ui, list)[0];
    assert_eq!(
        kind_of(&ui, children_of(&ui, content)[0]),
        WidgetKind::ListRow
    );
}

/// **The record is the frame's**: a node built as a button one frame and as
/// a plain block the next records nothing the second time.
#[test]
fn a_node_built_plain_after_a_widget_records_nothing() {
    let mut ui = Ui::new();
    let key = frame(&mut ui, idle(), NavInput::default(), |ui| {
        ui.button("#same", "Same").key
    });
    assert_eq!(kind_of(&ui, key), WidgetKind::Button);
    let again = frame(&mut ui, idle(), NavInput::default(), |ui| {
        ui.block("button#same", &[], |_| {}).key
    });
    assert_eq!(again, key, "the two builds are one node");
    assert_eq!(kind_of(&ui, key), WidgetKind::None);
}
