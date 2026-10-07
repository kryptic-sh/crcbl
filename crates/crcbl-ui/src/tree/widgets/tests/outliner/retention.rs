use super::*;

#[test]
fn expansion_release_respects_a_model_refreshed_during_the_press() {
    for removed in [false, true] {
        let mut ui = Ui::new();
        sheet(&mut ui);
        let mut state = OutlinerState::new();
        let step = |ui: &mut Ui, state: &mut OutlinerState, pointer, present| {
            page(
                ui,
                state,
                pointer,
                NavInput::default(),
                SelectMode::Replace,
                |out| {
                    if present {
                        fixture(out);
                    }
                },
            )
        };
        step(&mut ui, &mut state, idle(), true);
        let built = step(&mut ui, &mut state, idle(), true);
        let root = row_key(&ui, &built, id(0));
        let at = centre(&ui, children_of(&ui, root)[0]);
        step(&mut ui, &mut state, press(at), true);

        state.retain(|_| !removed);
        let released = step(&mut ui, &mut state, release(at), !removed);
        assert_eq!(
            state.is_expanded(id(0)),
            !removed,
            "release must only expand a surviving branch"
        );
        assert_eq!(released.outliner.changed, !removed);
        if removed {
            assert!(state.rows().is_empty());
        } else {
            assert_eq!(flat(&state), [0, 1, 2, 5]);
        }
    }
}

#[test]
fn keyboard_expansion_requires_a_branch_in_the_refreshed_model() {
    for becomes_leaf in [false, true] {
        let mut ui = Ui::new();
        sheet(&mut ui);
        let mut state = OutlinerState::new();
        fixture_page(&mut ui, &mut state, idle(), NavInput::default());
        let built = fixture_page(&mut ui, &mut state, idle(), NavInput::NAVIGATION);
        assert_eq!(ui.focused(), Some(row_key(&ui, &built, id(0))));

        state.retain(|_| becomes_leaf);
        let refreshed = page(
            &mut ui,
            &mut state,
            idle(),
            RIGHT,
            SelectMode::Replace,
            |out| {
                if becomes_leaf {
                    out.leaf(id(0));
                }
            },
        );
        assert!(!state.is_expanded(id(0)));
        assert!(!refreshed.outliner.changed);
        assert_eq!(state.rows().len(), usize::from(becomes_leaf));
    }
}
