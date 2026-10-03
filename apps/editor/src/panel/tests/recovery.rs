//! The recovery bar: what it says, the answer each button gives, and the
//! panels going on working under it.

use super::*;

/// A page with the recovery bar up offering two copies, laid out.
fn offering() -> Page {
    let mut page = Page::built_in();
    page.panels.begin_recovery(
        "An earlier run left 2 recovery copies".to_owned(),
        vec![
            "`greybox`, 1 min ago".to_owned(),
            "`field`, 2 days ago".to_owned(),
        ],
    );
    page.idle();
    page
}

/// **Each of the bar's buttons hands back its answer**, naming the row, and
/// the bar says what it was told.
#[test]
fn each_button_on_the_recovery_bar_hands_back_its_answer() {
    let mut page = offering();
    let (heading, rows) = page.panels.recovery().expect("the bar is up");
    assert_eq!(heading, "An earlier run left 2 recovery copies");
    assert_eq!(rows, ["`greybox`, 1 min ago", "`field`, 2 days ago"]);
    let (buttons, later) = page.panels.recovery_buttons();
    let expected = [
        (buttons[0][0], RecoveryAnswer::Open(0)),
        (buttons[1][0], RecoveryAnswer::Open(1)),
        (buttons[0][1], RecoveryAnswer::Delete(0)),
        (buttons[1][1], RecoveryAnswer::Delete(1)),
        (later, RecoveryAnswer::Later),
    ];
    for (key, answer) in expected {
        let mut page = offering();
        let at = page.centre(key);
        let mut answers = Vec::new();
        for pointer in [
            PointerInput {
                pos: at,
                down: true,
                released: false,
                secondary_pressed: false,
            },
            PointerInput {
                pos: at,
                down: false,
                released: true,
                secondary_pressed: false,
            },
            PointerInput::hovering(Vec2::splat(-1.0)),
        ] {
            answers.extend(page.frame(pointer, 0.0).recovery);
        }
        assert_eq!(answers, [answer]);
    }
    page.panels.end_recovery();
    assert_eq!(page.panels.recovery(), None);
}

/// **The bar holds nothing**: with it up, a click on an outliner row still
/// selects — it is an offer, not a question.
#[test]
fn the_panels_go_on_working_under_the_recovery_bar() {
    let mut page = offering();
    let at = page.centre(page.panels.row_keys()[1]);
    page.click(at);
    assert!(
        page.document.primary().is_some(),
        "a click under the recovery bar selected nothing"
    );
    assert!(
        page.panels.recovery().is_some(),
        "the click took the bar down"
    );
}
