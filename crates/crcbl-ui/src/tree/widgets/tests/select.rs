//! The drop-down: opened by a click or accept, its list hanging under the
//! button as wide as it, picked by a click or by moves and accept, closed by
//! back with nothing picked — and one change reported per pick. A list longer
//! than the viewport scrolls; Home, End, the page keys and typeahead move
//! through it, and the arrows stop at its ends.

use std::time::Duration;

use super::*;
use crate::edit::Edit;
use crate::tree::{Jump, POPUP_MARGIN, Response, TYPEAHEAD_TIMEOUT, Ui};

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

/// How many options the long list offers: far more than [`PAGE`] holds.
const LONG: usize = 60;

/// Every frame's length unless a test says otherwise.
const FRAME: Duration = Duration::from_millis(16);

/// The long list's options, `option 00` up.
fn long_options() -> Vec<String> {
    (0..LONG)
        .map(|index| format!("option {index:02}"))
        .collect()
}

/// One frame of a page holding a drop-down over `options` at `chosen`, with
/// `text` as the frame's text input.
fn list_page(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    text: TextInput,
    options: &[&str],
    chosen: &mut usize,
) -> Response {
    frame_with_text(ui, pointer, nav, text, |ui| {
        ui.select("#list", options, chosen)
    })
}

/// A frame's text input typing `text`, `dt` long.
fn typing(text: &str, dt: Duration) -> TextInput {
    TextInput {
        dt,
        edits: vec![Edit::Insert(text.to_owned())],
        clipboard: Vec::new(),
    }
}

/// A frame's text input of nothing, `dt` long.
fn quiet(dt: Duration) -> TextInput {
    TextInput {
        dt,
        ..TextInput::default()
    }
}

/// Whether `key`'s border box lies wholly inside `list`'s content box, as
/// last laid out.
fn shown_in(ui: &Ui, list: NodeKey, key: NodeKey) -> bool {
    let (min, max) = rect(ui, key);
    let (view_min, view_max) = ui.store.by_key(list).expect("laid out").content_box();
    min.cmpge(view_min).all() && max.cmple(view_max).all()
}

/// A drop-down over `options`, focused and opened by accept at `chosen`, its
/// list laid out and focus inside it. Returns the drop-down's key.
fn opened_list(ui: &mut Ui, options: &[&str], chosen: &mut usize) -> NodeKey {
    list_page(
        ui,
        idle(),
        NavInput::default(),
        quiet(FRAME),
        options,
        chosen,
    );
    let select = list_page(ui, idle(), NavInput::NEXT, quiet(FRAME), options, chosen).key;
    list_page(ui, idle(), NavInput::ACCEPT, quiet(FRAME), options, chosen);
    list_page(
        ui,
        idle(),
        NavInput::NAVIGATION,
        quiet(FRAME),
        options,
        chosen,
    );
    assert!(ui.is_popup_open(select), "accept did not open the list");
    select
}

/// **A list longer than the viewport is capped at the viewport less
/// [`POPUP_MARGIN`], and the wheel over it scrolls it**: the options move up
/// by what the wheel turned, and the list stays where it is.
#[test]
fn a_long_list_is_capped_to_the_viewport_and_scrolls_with_the_wheel() {
    let owned = long_options();
    let options: Vec<&str> = owned.iter().map(String::as_str).collect();
    let mut ui = Ui::new();
    let mut chosen = 0;
    let select = opened_list(&mut ui, &options, &mut chosen);
    let list = Ui::popup_key(select);
    let (list_min, list_max) = rect(&ui, list);
    assert_eq!(
        list_max.y - list_min.y,
        PAGE - POPUP_MARGIN,
        "the list was not capped at the viewport"
    );
    assert!(list_max.y <= PAGE, "the capped list runs off the viewport");

    let first = option_key(&ui, select, "option 00");
    let before = rect(&ui, first).0.y;
    let over = (list_min + list_max) * 0.5;
    frame(
        &mut ui,
        PointerInput::hovering(over),
        NavInput::default(),
        |ui| {
            assert!(
                ui.scroll_wheel(Vec2::new(0.0, 30.0)),
                "the wheel over the list moved nothing"
            );
            ui.select("#list", &options, &mut chosen);
        },
    );
    assert_eq!(
        rect(&ui, first).0.y,
        before - 30.0,
        "the options did not move"
    );
    assert_eq!(
        rect(&ui, list),
        (list_min, list_max),
        "the list itself moved"
    );
}

/// **Arrowing past the last option in view scrolls the next one into view.**
#[test]
fn arrowing_past_the_visible_end_reveals_the_option() {
    let owned = long_options();
    let options: Vec<&str> = owned.iter().map(String::as_str).collect();
    let mut ui = Ui::new();
    let mut chosen = 0;
    let select = opened_list(&mut ui, &options, &mut chosen);
    let list = Ui::popup_key(select);
    let hidden = (0..LONG)
        .find(|&index| !shown_in(&ui, list, option_key(&ui, select, options[index])))
        .expect("the long list shows every option");
    for _ in 0..hidden {
        list_page(&mut ui, idle(), DOWN, quiet(FRAME), &options, &mut chosen);
    }
    let reached = option_key(&ui, select, options[hidden]);
    assert_eq!(ui.focused(), Some(reached), "the moves did not reach it");
    assert!(
        shown_in(&ui, list, reached),
        "the option arrowed onto is out of view"
    );
}

/// **The list opens scrolled to the chosen option**, drawn in view in the
/// frame it opens rather than the frame after.
#[test]
fn the_chosen_option_is_shown_in_the_frame_the_list_opens() {
    let owned = long_options();
    let options: Vec<&str> = owned.iter().map(String::as_str).collect();
    let mut ui = Ui::new();
    let mut chosen = LONG - 3;
    let quiet = || quiet(FRAME);
    list_page(
        &mut ui,
        idle(),
        NavInput::default(),
        quiet(),
        &options,
        &mut chosen,
    );
    let select = list_page(
        &mut ui,
        idle(),
        NavInput::NEXT,
        quiet(),
        &options,
        &mut chosen,
    )
    .key;
    list_page(
        &mut ui,
        idle(),
        NavInput::ACCEPT,
        quiet(),
        &options,
        &mut chosen,
    );
    assert!(ui.is_popup_open(select));
    let list = Ui::popup_key(select);
    assert!(
        shown_in(&ui, list, option_key(&ui, select, options[chosen])),
        "the opening frame drew the chosen option out of view"
    );
}

/// **Home and End go to the first and last option, Page Down and Page Up a
/// view's worth**, each scrolled into view, and none of them picks.
#[test]
fn home_end_and_the_page_keys_move_through_the_list() {
    let owned = long_options();
    let options: Vec<&str> = owned.iter().map(String::as_str).collect();
    let mut ui = Ui::new();
    let mut chosen = 0;
    let select = opened_list(&mut ui, &options, &mut chosen);
    let list = Ui::popup_key(select);
    let mut jump = |ui: &mut Ui, jump: Jump| {
        list_page(
            ui,
            idle(),
            NavInput::jumping(jump),
            quiet(FRAME),
            &options,
            &mut chosen,
        );
        let focused = ui.focused().expect("focus is in the list");
        let index = (0..LONG)
            .find(|&index| option_key(ui, select, options[index]) == focused)
            .expect("focus left the list");
        assert!(
            shown_in(ui, list, focused),
            "{jump:?} left focus out of view"
        );
        index
    };

    let (first_min, first_max) = rect(&ui, option_key(&ui, select, options[0]));
    let height = first_max.y - first_min.y;
    let (view_min, view_max) = ui.store.by_key(list).expect("laid out").content_box();
    // A whole number of options: the cast only drops the fraction.
    let per_page = ((view_max.y - view_min.y) / height).floor() as usize;
    assert!(per_page > 1, "the fixture's page holds one option");

    assert_eq!(jump(&mut ui, Jump::PageDown), per_page);
    assert_eq!(jump(&mut ui, Jump::PageDown), 2 * per_page);
    assert_eq!(jump(&mut ui, Jump::PageUp), per_page);
    assert_eq!(jump(&mut ui, Jump::Last), LONG - 1);
    assert_eq!(jump(&mut ui, Jump::PageDown), LONG - 1);
    assert_eq!(jump(&mut ui, Jump::First), 0);
    assert_eq!(jump(&mut ui, Jump::PageUp), 0);
    assert_eq!(chosen, 0, "a jump picked");
}

/// **The arrows stop at the list's ends**: down on the last option and up on
/// the first leave focus where it is, as a list box's do.
#[test]
fn the_arrows_stop_at_the_lists_ends() {
    let mut ui = Ui::new();
    let mut chosen = OPTIONS.len() - 1;
    let select = opened_list(&mut ui, &OPTIONS, &mut chosen);
    let last = option_key(&ui, select, "high");
    assert_eq!(ui.focused(), Some(last));
    list_page(&mut ui, idle(), DOWN, quiet(FRAME), &OPTIONS, &mut chosen);
    assert_eq!(ui.focused(), Some(last), "down wrapped off the last option");
    let home = NavInput::jumping(Jump::First);
    list_page(&mut ui, idle(), home, quiet(FRAME), &OPTIONS, &mut chosen);
    let first = option_key(&ui, select, "low");
    assert_eq!(ui.focused(), Some(first));
    list_page(&mut ui, idle(), UP, quiet(FRAME), &OPTIONS, &mut chosen);
    assert_eq!(ui.focused(), Some(first), "up wrapped off the first option");
}

/// The options typeahead is tried on.
const FRUIT: [&str; 5] = ["apple", "Apricot", "banana", "blueberry", "cherry"];

/// One frame typing `text`, `dt` after the last, into the open fruit list.
fn type_into(ui: &mut Ui, text: &str, dt: Duration, chosen: &mut usize) {
    list_page(
        ui,
        idle(),
        NavInput::NAVIGATION,
        typing(text, dt),
        &FRUIT,
        chosen,
    );
}

/// One frame of nothing over the fruit list, `dt` long.
fn wait(ui: &mut Ui, dt: Duration, chosen: &mut usize) {
    list_page(ui, idle(), NavInput::NAVIGATION, quiet(dt), &FRUIT, chosen);
}

/// Which fruit focus is on.
fn focused_fruit(ui: &Ui, select: NodeKey) -> &'static str {
    FRUIT
        .into_iter()
        .find(|fruit| ui.focused() == Some(option_key(ui, select, fruit)))
        .expect("focus is on no option")
}

/// **Typed letters move focus to the option they begin, ignoring case**: `b`
/// reaches banana, `l` after it blueberry, `AP` apple — each focused the
/// frame after it is typed — and the list takes the keys while it is open.
#[test]
fn typeahead_jumps_to_the_option_the_typed_letters_begin() {
    let mut ui = Ui::new();
    let mut chosen = FRUIT.len() - 1;
    let select = opened_list(&mut ui, &FRUIT, &mut chosen);
    assert!(ui.popup_list_open(), "the open list does not take the keys");
    type_into(&mut ui, "b", FRAME, &mut chosen);
    type_into(&mut ui, "l", FRAME, &mut chosen);
    assert_eq!(
        focused_fruit(&ui, select),
        "banana",
        "the b was not followed"
    );
    wait(&mut ui, FRAME, &mut chosen);
    assert_eq!(focused_fruit(&ui, select), "blueberry");
    wait(&mut ui, TYPEAHEAD_TIMEOUT, &mut chosen);
    type_into(&mut ui, "AP", FRAME, &mut chosen);
    wait(&mut ui, FRAME, &mut chosen);
    assert_eq!(focused_fruit(&ui, select), "apple");
    assert_eq!(chosen, FRUIT.len() - 1, "typing picked");

    list_page(
        &mut ui,
        idle(),
        NavInput::BACK,
        quiet(FRAME),
        &FRUIT,
        &mut chosen,
    );
    assert!(!ui.popup_list_open(), "a closed list still takes the keys");
}

/// **A letter typed again and again steps through the options it begins**,
/// wrapping round, rather than looking for `aa`.
#[test]
fn typeahead_cycles_on_a_repeated_letter() {
    let mut ui = Ui::new();
    let mut chosen = 0;
    let select = opened_list(&mut ui, &FRUIT, &mut chosen);
    let mut seen = Vec::new();
    for _ in 0..3 {
        type_into(&mut ui, "a", FRAME, &mut chosen);
        wait(&mut ui, FRAME, &mut chosen);
        seen.push(focused_fruit(&ui, select));
    }
    assert_eq!(seen, ["Apricot", "apple", "Apricot"]);
}

/// **The prefix starts again after [`TYPEAHEAD_TIMEOUT`]** on the text clock:
/// `b` then `a` within it is `ba`, which stays on banana; `a` typed after it
/// is a search of its own, which finds apple.
#[test]
fn typeahead_starts_again_after_the_timeout() {
    let soon = TYPEAHEAD_TIMEOUT / 2;
    let late = TYPEAHEAD_TIMEOUT + FRAME;
    for (pause, expected) in [(soon, "banana"), (late, "apple")] {
        let mut ui = Ui::new();
        let mut chosen = 0;
        let select = opened_list(&mut ui, &FRUIT, &mut chosen);
        type_into(&mut ui, "b", FRAME, &mut chosen);
        wait(&mut ui, pause - FRAME, &mut chosen);
        type_into(&mut ui, "a", FRAME, &mut chosen);
        wait(&mut ui, FRAME, &mut chosen);
        assert_eq!(focused_fruit(&ui, select), expected, "after {pause:?}");
    }
}
