use super::*;

#[test]
fn disabled_pointer_input_does_not_expand_or_select_rows() {
    for toggle in [true, false] {
        let mut ui = Ui::new();
        sheet(&mut ui);
        let mut state = OutlinerState::new();
        let step = |ui: &mut Ui, state: &mut OutlinerState, pointer, enabled| {
            frame(ui, pointer, NavInput::default(), |ui| {
                let mut response = None;
                ui.enabled(enabled, |ui| {
                    response = Some(ui.outliner_with(
                        "#tree",
                        state,
                        &options(SelectMode::Replace),
                        fixture,
                        |ui, _| {
                            ui.span(".label", "x", &[]);
                        },
                    ));
                });
                response.expect("built")
            })
        };
        step(&mut ui, &mut state, idle(), true);
        let first = step(&mut ui, &mut state, idle(), true);
        let content = children_of(&ui, first.key)[0];
        let root = children_of(&ui, content)[0];
        let at = if toggle {
            centre(&ui, children_of(&ui, root)[0])
        } else {
            let (min, max) = rect(&ui, root);
            Vec2::new(max.x - 2.0, (min.y + max.y) * 0.5)
        };

        step(&mut ui, &mut state, press(at), true);
        let released = step(&mut ui, &mut state, release(at), false);
        assert!(!released.changed, "a disabled release changed the outliner");
        assert!(!state.is_expanded(id(0)));
        assert_eq!(state.selected_len(), 0);
        assert_eq!(state.double_clicked(), None);

        step(&mut ui, &mut state, press(at), false);
        let disabled = step(&mut ui, &mut state, release(at), false);
        assert!(!disabled.changed, "a disabled click changed the outliner");
        assert!(!state.is_expanded(id(0)));
        assert_eq!(state.selected_len(), 0);
        assert_eq!(state.double_clicked(), None);

        let enabled = step(&mut ui, &mut state, idle(), true);
        assert!(!enabled.changed, "re-enabling replayed a disabled click");
        assert!(!state.is_expanded(id(0)));
        assert_eq!(state.selected_len(), 0);
        step(&mut ui, &mut state, press(at), true);
        let clicked = step(&mut ui, &mut state, release(at), true);
        assert!(clicked.changed, "re-enabling did not restore clicks");
        assert_eq!(state.is_expanded(id(0)), toggle);
        assert_eq!(state.is_selected(id(0)), !toggle);
        assert_eq!(state.double_clicked(), None, "a disabled click was counted");
    }
}
