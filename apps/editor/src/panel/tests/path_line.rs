//! The path line: a directory typed under the toolbar for a save-as or an
//! open, committed on accept and dropped on back, and refused in play mode.

use super::*;

use crcbl::ui::edit::Edit;

/// One still frame with `nav` and `text`, handing back what it did.
fn frame_typing(page: &mut Page, nav: NavInput, text: TextInput) -> PanelFrame {
    page.panels.frame(
        &mut page.document,
        PanelInput {
            pointer: PointerInput::hovering(Vec2::splat(-1.0)),
            nav,
            text,
            extent: EXTENT,
            select: SelectMode::Replace,
            scroll: 0.0,
        },
    )
}

/// A still frame typing `text`.
fn type_text(page: &mut Page, text: &str) -> PanelFrame {
    frame_typing(
        page,
        NavInput::default(),
        TextInput {
            edits: vec![Edit::Insert(text.to_owned())],
            ..TextInput::default()
        },
    )
}

/// A still frame pressing accept, or back.
fn press_nav(page: &mut Page, accept: bool) -> PanelFrame {
    let nav = NavInput {
        accept,
        back: !accept,
        ..NavInput::default()
    };
    frame_typing(page, nav, TextInput::default())
}

/// **The save-as line takes the next key typed and hands back what was typed
/// on accept**, closing — and back closes it handing back nothing. Neither
/// writes anything: saving is the caller's.
#[test]
fn the_save_as_line_types_a_directory_and_commits_or_cancels() {
    let mut page = Page::built_in();
    page.idle();
    assert_eq!(page.panels.saving_as(), None);

    page.panels
        .begin_save_as(&page.document, String::new())
        .expect("editing");
    assert_eq!(page.panels.saving_as(), Some(""));
    page.idle();
    assert!(
        page.panels.text_editing(),
        "the line's input was not engaged, so a key typed now is not its"
    );
    let typed = type_text(&mut page, "levels/one.scn");
    assert_eq!(typed.save_as, None, "a keystroke committed it");
    assert_eq!(page.panels.saving_as(), Some("levels/one.scn"));
    let committed = press_nav(&mut page, true);
    assert_eq!(committed.save_as.as_deref(), Some("levels/one.scn"));
    assert_eq!(page.panels.saving_as(), None, "the line stayed open");

    page.panels
        .begin_save_as(&page.document, "kept.scn".to_owned())
        .expect("editing");
    page.idle();
    type_text(&mut page, "x");
    let cancelled = press_nav(&mut page, false);
    assert_eq!(cancelled.save_as, None, "back committed it");
    assert_eq!(page.panels.saving_as(), None, "back left the line open");
    assert!(!page.panels.text_editing());
}

/// **The save-as line is refused in play mode**, and opens nothing.
#[test]
fn the_save_as_line_is_refused_in_play_mode() {
    let mut page = Page::built_in();
    page.document.play().expect("the greybox scene plays");
    assert!(matches!(
        page.panels.begin_save_as(&page.document, String::new()),
        Err(EditError::Playing)
    ));
    assert_eq!(page.panels.saving_as(), None);
}

/// **The line opened for an open hands back what was typed as an open**, not
/// a save-as — and opening it for one replaces the other.
#[test]
fn the_open_line_commits_an_open() {
    let mut page = Page::built_in();
    page.panels
        .begin_save_as(&page.document, "saved.scn".to_owned())
        .expect("editing");
    page.panels
        .begin_open(&page.document, String::new())
        .expect("editing");
    assert_eq!(page.panels.saving_as(), None, "the save-as stayed open");
    assert_eq!(page.panels.opening(), Some(""));
    page.idle();
    type_text(&mut page, "levels/one.scn");
    let committed = press_nav(&mut page, true);
    assert_eq!(committed.open.as_deref(), Some("levels/one.scn"));
    assert_eq!(committed.save_as, None, "an open was handed back as a save");
    assert_eq!(page.panels.opening(), None, "the line stayed open");
}

/// **The open line is refused in play mode**, and opens nothing.
#[test]
fn the_open_line_is_refused_in_play_mode() {
    let mut page = Page::built_in();
    page.document.play().expect("the greybox scene plays");
    assert!(matches!(
        page.panels.begin_open(&page.document, String::new()),
        Err(EditError::Playing)
    ));
    assert_eq!(page.panels.opening(), None);
}
