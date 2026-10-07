use super::*;

#[test]
fn vertical_range_steps_extend_and_shrink_around_the_anchor() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let mut state = OutlinerState::new();
    state.set_expanded(id(0), true);
    state.select(id(1), SelectMode::Replace);
    let built = fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    ui.set_focus(row_key(&ui, &built, id(1)));
    fixture_page(&mut ui, &mut state, idle(), NavInput::default());

    let extended = page(
        &mut ui,
        &mut state,
        idle(),
        DOWN,
        SelectMode::Range,
        fixture,
    );
    assert_eq!(state.selected().collect::<Vec<_>>(), [id(1), id(2)]);
    assert_eq!(state.anchor(), Some(id(1)));
    assert!(extended.outliner.changed);

    page(
        &mut ui,
        &mut state,
        idle(),
        DOWN,
        SelectMode::Range,
        fixture,
    );
    assert_eq!(state.selected().collect::<Vec<_>>(), [id(1), id(2), id(5)]);
    assert!(
        !state.is_selected(id(3)),
        "a collapsed descendant joined the range"
    );

    page(&mut ui, &mut state, idle(), UP, SelectMode::Range, fixture);
    assert_eq!(state.selected().collect::<Vec<_>>(), [id(1), id(2)]);
    page(&mut ui, &mut state, idle(), UP, SelectMode::Range, fixture);
    assert_eq!(state.selected().collect::<Vec<_>>(), [id(1)]);
    assert_eq!(state.anchor(), Some(id(1)));

    let plain = fixture_page(&mut ui, &mut state, idle(), DOWN);
    assert_eq!(state.selected().collect::<Vec<_>>(), [id(1)]);
    assert!(!plain.outliner.changed, "plain navigation selected a row");
    page(
        &mut ui,
        &mut state,
        idle(),
        RIGHT,
        SelectMode::Range,
        fixture,
    );
    assert!(state.is_expanded(id(2)));
    assert_eq!(state.selected().collect::<Vec<_>>(), [id(1)]);
}

#[test]
fn a_range_step_without_an_anchor_starts_at_the_previous_focus() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let mut state = OutlinerState::new();
    state.set_expanded(id(0), true);
    let built = fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    ui.set_focus(row_key(&ui, &built, id(1)));
    fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    page(
        &mut ui,
        &mut state,
        idle(),
        DOWN,
        SelectMode::Range,
        fixture,
    );
    assert_eq!(state.anchor(), Some(id(1)));
    assert_eq!(state.selected().collect::<Vec<_>>(), [id(1), id(2)]);
    state.select(id(5), SelectMode::Replace);
    page(
        &mut ui,
        &mut state,
        idle(),
        NavInput::default(),
        SelectMode::Range,
        fixture,
    );
    assert_eq!(
        state.selected().collect::<Vec<_>>(),
        [id(5)],
        "an old movement was applied again"
    );
}

#[test]
fn disabling_an_outliner_before_building_it_prevents_range_selection() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let mut state = OutlinerState::new();
    state.set_expanded(id(0), true);
    state.select(id(1), SelectMode::Replace);
    let built = fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    ui.set_focus(row_key(&ui, &built, id(1)));
    fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    frame(&mut ui, idle(), DOWN, |ui| {
        ui.enabled(false, |ui| {
            ui.outliner_with(
                "#tree",
                &mut state,
                &options(SelectMode::Range),
                fixture,
                |ui, _| {
                    ui.span(".label", "x", &[]);
                },
            );
        });
    });
    assert_eq!(state.selected().collect::<Vec<_>>(), [id(1)]);
    assert_eq!(state.anchor(), Some(id(1)));
}

#[test]
fn moving_between_outliners_does_not_start_a_range() {
    let mut ui = Ui::new();
    ui.add_stylesheet("trees.css", "outliner { height: 40px; }");
    let mut states = [OutlinerState::new(), OutlinerState::new()];
    let build = |ui: &mut Ui, states: &mut [OutlinerState; 2], nav| {
        frame(ui, idle(), nav, |ui| {
            let [first, second] = states;
            [("#first", first), ("#second", second)].map(|(selector, state)| {
                ui.outliner_with(
                    selector,
                    state,
                    &options(SelectMode::Range),
                    |out| out.leaf(id(1)),
                    |ui, _| {
                        ui.span(".label", "x", &[]);
                    },
                )
            })
        })
    };
    let built = build(&mut ui, &mut states, NavInput::default());
    let keys = built.map(|response| {
        let content = children_of(&ui, response.key)[0];
        children_of(&ui, content)[0]
    });
    ui.set_focus(keys[0]);
    build(&mut ui, &mut states, NavInput::default());
    build(&mut ui, &mut states, DOWN);
    assert_eq!(
        ui.focused(),
        Some(keys[1]),
        "focus did not enter the other outliner"
    );
    assert_eq!(states[0].selected_len(), 0);
    assert_eq!(states[1].selected_len(), 0);
}

#[test]
fn a_range_step_does_not_use_rows_removed_during_the_frame() {
    for removed in [id(1), id(2)] {
        let mut ui = Ui::new();
        sheet(&mut ui);
        let mut state = OutlinerState::new();
        state.set_expanded(id(0), true);
        let built = fixture_page(&mut ui, &mut state, idle(), NavInput::default());
        ui.set_focus(row_key(&ui, &built, id(1)));
        fixture_page(&mut ui, &mut state, idle(), NavInput::default());
        state.invalidate();
        page(
            &mut ui,
            &mut state,
            idle(),
            DOWN,
            SelectMode::Range,
            |out| {
                out.branch(id(0), |out| {
                    for item in [id(1), id(2)] {
                        if item != removed {
                            out.leaf(item);
                        }
                    }
                });
            },
        );
        assert_eq!(
            state.selected_len(),
            0,
            "a range used removed row {removed:?}"
        );
        assert_eq!(state.anchor(), None);
    }
}
