//! The drop-down: opened by a click or accept, its list hanging under the
//! button as wide as it, picked by a click or by moves and accept, closed by
//! back with nothing picked — and one change reported per pick.

use super::*;
use crate::tree::{Response, Ui};

/// The options every page offers.
const OPTIONS: [&str; 3] = ["low", "mid", "high"];

/// One frame of a page holding a drop-down over [`OPTIONS`] at `chosen`,
/// enabled as `enabled` says.
fn page(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    chosen: &mut usize,
    enabled: bool,
) -> Response {
    frame(ui, pointer, nav, |ui| {
        let mut response = None;
        ui.enabled(enabled, |ui| {
            response = Some(ui.select("#quality", &OPTIONS, chosen));
        });
        response.expect("built")
    })
}

/// The key of the list's option reading `text`: the parent of its label.
fn option_key(ui: &Ui, select: NodeKey, text: &str) -> NodeKey {
    children_of(ui, Ui::popup_key(select))
        .into_iter()
        .find(|&option| {
            children_of(ui, option)
                .first()
                .is_some_and(|&label| ui.text(label) == Some(text))
        })
        .unwrap_or_else(|| panic!("the list holds no {text:?}"))
}

/// **Moves and accept pick one option, reported as one change**: accept on
/// the focused drop-down opens the list on the chosen option, a move down
/// reaches the next, accept picks it and closes the list — the one frame
/// whose response is changed — and focus is back on the button.
#[test]
fn moves_and_accept_pick_one_option_reported_as_one_change() {
    let mut ui = Ui::new();
    let mut chosen = 0;
    let mut changes = 0;
    let mut step = |ui: &mut Ui, nav: NavInput, chosen: &mut usize| {
        let response = page(ui, idle(), nav, chosen, true);
        changes += usize::from(response.changed);
        response
    };
    step(&mut ui, NavInput::default(), &mut chosen);
    let select = step(&mut ui, NavInput::NEXT, &mut chosen).key;
    assert_eq!(ui.focused(), Some(select));
    step(&mut ui, NavInput::ACCEPT, &mut chosen);
    assert!(ui.is_popup_open(select), "accept did not open the list");
    step(&mut ui, NavInput::NAVIGATION, &mut chosen);
    assert_eq!(
        ui.focused(),
        Some(option_key(&ui, select, "low")),
        "the list did not take focus on the chosen option"
    );
    step(&mut ui, DOWN, &mut chosen);
    assert_eq!(ui.focused(), Some(option_key(&ui, select, "mid")));
    assert_eq!(chosen, 0, "a move picked");

    let picked = step(&mut ui, NavInput::ACCEPT, &mut chosen);
    assert!(picked.changed, "the pick's frame did not report it");
    assert_eq!(chosen, 1);
    assert!(!ui.is_popup_open(select), "the pick left the list open");
    step(&mut ui, NavInput::NAVIGATION, &mut chosen);
    assert_eq!(ui.focused(), Some(select), "focus did not return");
    assert_eq!(changes, 1, "one pick was not one change");
}

/// **Back closes the list with nothing picked**, focus back on the button.
#[test]
fn back_closes_the_list_with_nothing_picked() {
    let mut ui = Ui::new();
    let mut chosen = 2;
    page(&mut ui, idle(), NavInput::default(), &mut chosen, true);
    let select = page(&mut ui, idle(), NavInput::NEXT, &mut chosen, true).key;
    page(&mut ui, idle(), NavInput::ACCEPT, &mut chosen, true);
    page(&mut ui, idle(), UP, &mut chosen, true);
    page(&mut ui, idle(), UP, &mut chosen, true);
    let closed = page(&mut ui, idle(), NavInput::BACK, &mut chosen, true);
    assert!(!closed.changed);
    assert_eq!(chosen, 2);
    assert!(!ui.is_popup_open(select));
    assert_eq!(ui.focused(), Some(select));
}

/// **A click opens the list under the button, at least as wide as it, and a
/// click on an option picks it.**
#[test]
fn a_click_opens_the_list_under_the_button_and_a_click_picks() {
    let mut ui = Ui::new();
    let mut chosen = 0;
    let select = page(&mut ui, idle(), NavInput::default(), &mut chosen, true).key;
    let on = centre(&ui, select);
    page(&mut ui, press(on), NavInput::default(), &mut chosen, true);
    page(&mut ui, release(on), NavInput::default(), &mut chosen, true);
    assert!(ui.is_popup_open(select), "the click did not open the list");
    let (button_min, button_max) = rect(&ui, select);
    let (list_min, list_max) = rect(&ui, Ui::popup_key(select));
    assert_eq!(list_min, Vec2::new(button_min.x, button_max.y));
    assert!(
        list_max.x - list_min.x >= button_max.x - button_min.x,
        "the list is narrower than its button"
    );

    let on = centre(&ui, option_key(&ui, select, "high"));
    page(&mut ui, press(on), NavInput::default(), &mut chosen, true);
    let picked = page(&mut ui, release(on), NavInput::default(), &mut chosen, true);
    assert!(picked.changed);
    assert_eq!(chosen, 2);
    assert!(!ui.is_popup_open(select));
}

/// **A disabled drop-down never opens — not even from a click that began
/// while it was enabled — and one disabled while its list is open closes
/// it.**
#[test]
fn a_disabled_drop_down_never_opens_and_disabling_one_closes_its_list() {
    let mut ui = Ui::new();
    let mut chosen = 0;
    let select = page(&mut ui, idle(), NavInput::default(), &mut chosen, false).key;
    let on = centre(&ui, select);
    page(&mut ui, press(on), NavInput::default(), &mut chosen, false);
    page(
        &mut ui,
        release(on),
        NavInput::default(),
        &mut chosen,
        false,
    );
    assert!(!ui.is_popup_open(select), "a disabled drop-down opened");
    // Pressed while enabled and released in the frame that disables it: the
    // click is resolved against the enabled node, and still opens nothing.
    page(&mut ui, press(on), NavInput::default(), &mut chosen, true);
    page(
        &mut ui,
        release(on),
        NavInput::default(),
        &mut chosen,
        false,
    );
    assert!(
        !ui.is_popup_open(select),
        "the frame that disabled it opened it"
    );

    page(&mut ui, press(on), NavInput::default(), &mut chosen, true);
    page(&mut ui, release(on), NavInput::default(), &mut chosen, true);
    assert!(
        ui.is_popup_open(select),
        "the enabled drop-down did not open"
    );
    page(&mut ui, idle(), NavInput::default(), &mut chosen, false);
    assert!(!ui.is_popup_open(select), "disabling it left its list open");
    assert_eq!(chosen, 0);
}
