//! `default.css` over the widget set: the focus ring, and a type outside the
//! set left alone.

use super::*;
use crate::tree::{InputMode, SplitAxis};

/// Every widget of the set on one page, and the key of each node focus can
/// rest on, in tree order.
fn every_widget(ui: &mut Ui, nav: NavInput, values: &mut (bool, f32, f32, String)) -> Vec<NodeKey> {
    frame(ui, idle(), nav, |ui| {
        let mut keys = vec![
            ui.button("#b", "B").key,
            ui.checkbox("#c", "C", &mut values.0).key,
            ui.slider("#s", &mut values.1, 0.0..=1.0, 0.1).key,
            ui.drag_value("#d", &mut values.2, 0.0..=1.0, 0.01, 0.1).key,
            ui.collapsing("#h", "H", |_| {}).key,
            ui.tree_leaf("#t", "T").key,
            ui.text_input("#i", &mut values.3).key,
        ];
        let divider = ui.split(
            "",
            SplitAxis::Row,
            [10.0, 10.0],
            |ui| {
                ui.list("#l", 1, 16.0, |ui, _| {
                    ui.span("", "row", &[]);
                });
            },
            |_| {},
        );
        keys.push(divider.key);
        keys
    })
}

/// **`default.css` rings exactly the focused widget, whichever widget it is,
/// and only while the pad or the keyboard is driving** — a two-pixel outline
/// on the node focus rests on, on every other focusable node none — and a
/// node whose selector names its own type gets no ring from the engine.
#[test]
fn the_default_sheet_rings_the_focused_widget_of_every_kind() {
    let ring = linear("#f5c400");
    let mut ui = Ui::new();
    let mut values = (false, 0.5, 0.5, String::new());
    let keys = every_widget(&mut ui, NavInput::default(), &mut values);
    let ringed = |ui: &Ui| -> Vec<NodeKey> {
        ui.nodes
            .iter()
            .filter(|node| node.style.outline_width > 0.0 && node.style.outline_color == ring)
            .map(|node| node.key)
            .collect()
    };

    every_widget(&mut ui, NavInput::NAVIGATION, &mut values);
    // Tree order walks every focusable node: each widget, then the list's
    // row, then the divider.
    let mut seen = Vec::new();
    for _ in 0..=keys.len() {
        let focused = ui.focused().expect("focus landed");
        assert_eq!(
            ringed(&ui),
            [focused],
            "not exactly the focused node ringed"
        );
        seen.push(focused);
        every_widget(&mut ui, NavInput::NEXT, &mut values);
    }
    for key in &keys {
        assert!(
            seen.contains(key),
            "a widget was never focused, so never ringed"
        );
    }
    assert_eq!(seen.len(), keys.len() + 1, "the list's row was not walked");

    every_widget(&mut ui, NavInput::default(), &mut values);
    assert_eq!(ui.input_mode(), InputMode::Pointer);
    assert!(ringed(&ui).is_empty(), "the ring showed for the pointer");

    let mut plain = Ui::new();
    let page = |ui: &mut Ui, nav| frame(ui, idle(), nav, |ui| ui.button("fancy#own", "Own").key);
    let own = page(&mut plain, NavInput::default());
    page(&mut plain, NavInput::NAVIGATION);
    assert_eq!(plain.focused(), Some(own));
    assert_eq!(
        style_of(&plain, own).outline_width,
        0.0,
        "a widget with its own type took the engine's ring"
    );
}
