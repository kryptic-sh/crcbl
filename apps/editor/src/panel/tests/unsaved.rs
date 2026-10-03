//! The unsaved bar: what it says, the answer each button gives, and the
//! panels holding still while it is up.

use super::*;

/// A press at `at` and its release, and the frame after — handing back what
/// each of the three frames did.
fn click_frames(page: &mut Page, at: Vec2) -> Vec<PanelFrame> {
    let mut frames = vec![page.frame(
        PointerInput {
            pos: at,
            down: true,
            released: false,
        },
        0.0,
    )];
    frames.push(page.frame(
        PointerInput {
            pos: at,
            down: false,
            released: true,
        },
        0.0,
    ));
    frames.push(page.idle());
    frames
}

/// **Each of the bar's buttons hands back its answer**, and the bar says what
/// it was told.
#[test]
fn each_button_on_the_bar_hands_back_its_answer() {
    for (index, answer) in [Unsaved::Save, Unsaved::Discard, Unsaved::Cancel]
        .into_iter()
        .enumerate()
    {
        let mut page = Page::built_in();
        page.panels
            .begin_unsaved("Unsaved edits to `greybox`".to_owned());
        assert_eq!(page.panels.unsaved(), Some("Unsaved edits to `greybox`"));
        page.idle();
        let at = page.centre(page.panels.unsaved_buttons()[index]);
        let answers: Vec<_> = click_frames(&mut page, at)
            .into_iter()
            .filter_map(|frame| frame.unsaved)
            .collect();
        assert_eq!(answers, [answer], "button {index}");
    }
}

/// **While the bar is up, the panels act on nothing else**: a click on an
/// outliner row selects nothing and takes no focus — and once the bar is down
/// a click on that row selects.
#[test]
fn the_panels_hold_still_while_the_bar_is_up() {
    let mut page = Page::built_in();
    page.panels.begin_unsaved("Unsaved edits".to_owned());
    page.idle();
    let at = page.centre(page.panels.row_keys()[1]);
    click_frames(&mut page, at);
    assert_eq!(
        page.document.primary(),
        None,
        "a click behind the bar selected"
    );
    assert!(
        !page.panels.holds_keyboard(),
        "a click behind the bar took focus"
    );

    page.panels.end_unsaved();
    assert_eq!(page.panels.unsaved(), None);
    // Laid out again without the bar, which moves every row up.
    page.idle();
    let at = page.centre(page.panels.row_keys()[1]);
    click_frames(&mut page, at);
    assert!(
        page.document.primary().is_some(),
        "the same click with the bar down selected nothing, so this shows nothing"
    );
}

/// **Putting the bar up closes a path line being typed, unsent**, so nothing
/// it held is saved or opened behind the question.
#[test]
fn the_bar_closes_a_path_line_unsent() {
    let mut page = Page::built_in();
    page.panels
        .begin_save_as(&page.document, "kept.scn".to_owned())
        .expect("editing");
    page.idle();
    page.idle();
    assert!(page.panels.text_editing());
    page.panels.begin_unsaved("Unsaved edits".to_owned());
    let frames = [page.idle(), page.idle()];
    assert_eq!(page.panels.saving_as(), None, "the line stayed open");
    assert!(
        frames.iter().all(|frame| frame.save_as.is_none()),
        "the line was committed"
    );
    assert!(!page.panels.text_editing());
}
