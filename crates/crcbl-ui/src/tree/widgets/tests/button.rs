//! The button and the checkbox: instant activation by pointer and by pad.

use super::*;
use crate::tree::{Engagement, Response};

/// **A button fires on a click and on accept through the one `clicked`, and
/// never engages** — for one frame each time, and a press that slides off it
/// before release fires nothing.
#[test]
fn a_button_fires_on_click_and_on_accept_and_never_engages() {
    let mut ui = Ui::new();
    let page = |ui: &mut Ui, pointer, nav| {
        frame(ui, pointer, nav, |ui| {
            let ok = ui.button("#ok", "OK");
            ui.button("#other", "Other");
            ok
        })
    };
    let ok = page(&mut ui, idle(), NavInput::default());
    let on = centre(&ui, ok.key);

    page(&mut ui, press(on), NavInput::default());
    let clicked = page(&mut ui, release(on), NavInput::default());
    assert!(clicked.clicked, "a click did not fire the button");
    assert!(
        !page(&mut ui, idle(), NavInput::default()).clicked,
        "a click fired for a second frame"
    );

    page(&mut ui, press(on), NavInput::default());
    let slid = page(&mut ui, release(Vec2::splat(390.0)), NavInput::default());
    assert!(!slid.clicked, "a press released off the button fired it");

    page(&mut ui, idle(), NavInput::NAVIGATION);
    let accepted = page(&mut ui, idle(), NavInput::ACCEPT);
    assert!(accepted.focused, "focus did not stay on the clicked button");
    assert!(accepted.clicked, "accept did not fire the focused button");
    assert_eq!(accepted.engagement, Engagement::Idle);
    assert_eq!(ui.engaged(), None, "a button engaged");
    assert!(
        !accepted.changed,
        "a button reported a change it has no value for"
    );
}

/// One frame of a page with a checkbox editing `value` above a button.
fn checkbox_page(ui: &mut Ui, pointer: PointerInput, nav: NavInput, value: &mut bool) -> Response {
    frame(ui, pointer, nav, |ui| {
        let response = ui.checkbox("#agree", "Agree", value);
        ui.button("#next", "Next");
        response
    })
}

/// **A checkbox flips its value on a click and on accept, reports the flip
/// in the frame it happens and no other, and is `:checked` exactly while its
/// value is true** — which `default.css` shows as the accent fill on its
/// mark.
#[test]
fn a_checkbox_flips_on_click_and_accept_and_is_checked_while_true() {
    let accent = linear("#3d8bfd");
    let mut ui = Ui::new();
    let mut value = false;
    let first = checkbox_page(&mut ui, idle(), NavInput::default(), &mut value);
    let mark = |ui: &Ui| {
        let boxed = children_of(ui, first.key)[0];
        style_of(ui, children_of(ui, boxed)[0]).background
    };
    assert_ne!(mark(&ui), accent, "an unchecked box shows its mark");

    let on = centre(&ui, first.key);
    checkbox_page(&mut ui, press(on), NavInput::default(), &mut value);
    assert!(!value, "the press alone flipped the value");
    let clicked = checkbox_page(&mut ui, release(on), NavInput::default(), &mut value);
    assert!(value, "a click did not check the box");
    assert!(clicked.changed, "the flip was not reported");
    assert_eq!(mark(&ui), accent, "`:checked` did not reach the mark");

    let quiet = checkbox_page(&mut ui, idle(), NavInput::default(), &mut value);
    assert!(value && !quiet.changed, "a frame with no input flipped it");

    checkbox_page(&mut ui, idle(), NavInput::NAVIGATION, &mut value);
    let accepted = checkbox_page(&mut ui, idle(), NavInput::ACCEPT, &mut value);
    assert!(!value, "accept did not uncheck the box");
    assert!(accepted.changed && accepted.engagement == Engagement::Idle);
    assert_ne!(mark(&ui), accent, "`:checked` stayed after unchecking");

    // A value the caller sets is shown without any input.
    value = true;
    checkbox_page(&mut ui, idle(), NavInput::default(), &mut value);
    assert_eq!(mark(&ui), accent, "the caller's value was not shown");
}

/// **A disabled checkbox is `:disabled`, cannot be focused, and a click on it
/// flips nothing.**
#[test]
fn a_disabled_checkbox_neither_focuses_nor_flips() {
    let mut ui = Ui::new();
    let mut value = false;
    let page = |ui: &mut Ui, pointer, nav, value: &mut bool| {
        frame(ui, pointer, nav, |ui| {
            let mut response = None;
            ui.enabled(false, |ui| {
                response = Some(ui.checkbox("#off", "Off", value));
            });
            response.expect("built")
        })
    };
    let off = page(&mut ui, idle(), NavInput::default(), &mut value);
    let on = centre(&ui, off.key);
    page(&mut ui, press(on), NavInput::default(), &mut value);
    let clicked = page(&mut ui, release(on), NavInput::default(), &mut value);
    assert!(!value && !clicked.changed, "a disabled checkbox flipped");
    page(&mut ui, idle(), NavInput::NAVIGATION, &mut value);
    assert_eq!(ui.focused(), None, "a disabled checkbox took focus");
    assert_eq!(
        style_of(&ui, off.key).color,
        linear("#7a8190"),
        "`:disabled` did not reach the checkbox's style"
    );
}
