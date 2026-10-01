//! Names in the outliner, renaming a row in place, and which inspector field
//! a clipboard key means.

use super::*;

use crcbl::scene::scn::EntityName;
use crcbl::ui::edit::Edit;

/// One frame with `nav` and `text` as well as the pointer — what typing into
/// a rename and committing it take.
fn frame_with(page: &mut Page, pointer: PointerInput, nav: NavInput, text: TextInput) {
    page.panels.frame(
        &mut page.document,
        PanelInput {
            pointer,
            nav,
            text,
            extent: EXTENT,
            select: SelectMode::Replace,
            scroll: 0.0,
        },
    );
}

/// A still frame typing `text`.
fn type_text(page: &mut Page, text: &str) {
    frame_with(
        page,
        PointerInput::hovering(Vec2::splat(-1.0)),
        NavInput::default(),
        TextInput {
            edits: vec![Edit::Insert(text.to_owned())],
            ..TextInput::default()
        },
    );
}

/// A still frame pressing accept, or back.
fn press_nav(page: &mut Page, accept: bool) {
    let nav = NavInput {
        accept,
        back: !accept,
        ..NavInput::default()
    };
    frame_with(
        page,
        PointerInput::hovering(Vec2::splat(-1.0)),
        nav,
        TextInput::default(),
    );
}

/// What each entity row of the outliner reads, in row order — the system's
/// header left out.
fn labels(page: &Page) -> Vec<String> {
    let ui = page.panels.ui();
    page.panels.row_keys()[1..]
        .iter()
        .map(|&row| {
            // A row is its toggle and then its label.
            let label = ui.child_keys(row)[1];
            ui.text(label).unwrap_or_default().to_owned()
        })
        .collect()
}

/// **A named entity's row reads its name and its id, and an unnamed one its
/// id alone** — and the row follows a rename, its undo and a delete, none of
/// which a still frame re-reading only the entity list would see.
#[test]
fn the_outliner_shows_names_and_follows_a_rename() {
    // Named before the panels exist, so their first frame has to read it.
    let mut document = Document::built_in().expect("the compiled-in scene");
    document.rename(SceneEntityId(0), "Ground").expect("held");
    let mut page = Page::over(document);
    page.idle();
    assert_eq!(labels(&page), ["Ground #0", "#1", "#2", "#3"]);
    page.document.undo().expect("the rename's inverse");
    page.idle();
    assert_eq!(labels(&page), ["#0", "#1", "#2", "#3"]);

    page.document
        .rename(SceneEntityId(2), "Gate")
        .expect("a held entity");
    page.idle();
    assert_eq!(labels(&page), ["#0", "#1", "Gate #2", "#3"]);

    page.document.undo().expect("the rename's inverse");
    page.idle();
    assert_eq!(labels(&page), ["#0", "#1", "#2", "#3"]);

    page.document.redo().expect("again");
    page.document.delete(SceneEntityId(1)).expect("held");
    page.idle();
    assert_eq!(labels(&page), ["#0", "Gate #2", "#3"]);
}

/// **A rename types into the row and commits on accept, as one command** —
/// the input engaged without a click, so the first key typed is the name's —
/// and back cancels one, leaving the name and the log as they were.
#[test]
fn a_rename_types_into_the_row_and_commits_or_cancels() {
    let mut page = Page::built_in();
    page.idle();
    let id = SceneEntityId(3);
    page.panels
        .begin_rename(&page.document, id)
        .expect("a held entity, while editing");
    assert_eq!(page.panels.renaming(), Some(id));
    page.idle();
    assert!(
        page.panels.text_editing(),
        "the rename's input was not engaged, so a key typed now is not its"
    );
    type_text(&mut page, "Spawner");
    assert_eq!(
        page.document.entity_name(id),
        None,
        "a keystroke renamed it"
    );
    press_nav(&mut page, true);
    page.idle();
    assert_eq!(
        page.document.entity_name(id).map(EntityName::as_str),
        Some("Spawner"),
    );
    assert_eq!(page.panels.renaming(), None);
    assert_eq!(page.document.log().len(), 1, "a rename is one command");
    assert!(labels(&page).contains(&"Spawner #3".to_owned()));

    // Begun again, typed over, and backed out of.
    page.panels.begin_rename(&page.document, id).expect("held");
    page.idle();
    type_text(&mut page, " Two");
    press_nav(&mut page, false);
    page.idle();
    assert_eq!(page.panels.renaming(), None);
    assert_eq!(
        page.document.entity_name(id).map(EntityName::as_str),
        Some("Spawner"),
        "a cancelled rename renamed it"
    );
    assert_eq!(page.document.log().len(), 1);
}

/// **A rename the document refuses is on the status line**, and changes
/// nothing: a control character cannot be typed, so the length limit is the
/// rule a row can break.
#[test]
fn a_refused_rename_is_on_the_status_line() {
    use crcbl::scene::scn::MAX_NAME_CHARS;

    let mut page = Page::built_in();
    page.idle();
    page.panels
        .begin_rename(&page.document, SceneEntityId(1))
        .expect("held");
    page.idle();
    type_text(&mut page, &"n".repeat(MAX_NAME_CHARS + 1));
    press_nav(&mut page, true);
    let (text, tone) = page.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains("at most"), "{text}");
    assert_eq!(page.document.entity_name(SceneEntityId(1)), None);
    assert!(page.document.log().is_empty());
}

/// **A double-click on a row starts renaming it**, and a single click does
/// not.
#[test]
fn a_double_click_on_a_row_starts_renaming_it() {
    let mut page = Page::built_in();
    page.idle();
    // Row 0 is the system's header; row 2 is the second entity.
    let at = page.centre(page.panels.row_keys()[2]);
    page.click(at);
    assert_eq!(page.document.selected(), Some(SceneEntityId(1)));
    assert_eq!(page.panels.renaming(), None, "one click started a rename");
    page.click(at);
    assert_eq!(page.panels.renaming(), Some(SceneEntityId(1)));
}

/// **A rename is refused in play mode before anything is put in the row.**
#[test]
fn a_rename_is_refused_in_play_mode() {
    let mut page = Page::built_in();
    page.document.play().expect("the greybox scene plays");
    page.idle();
    let error = page
        .panels
        .begin_rename(&page.document, SceneEntityId(1))
        .expect_err("play mode refuses edits");
    assert!(matches!(error, EditError::Playing), "{error}");
    assert_eq!(page.panels.renaming(), None);
}

/// **The field a clipboard key means is the focused leaf, else the one under
/// the pointer, else none**: hovering an axis names it, hovering the outliner
/// names nothing, a clicked field stays named once the pointer leaves, and
/// clicking an outliner row — which takes the keyboard — names nothing again.
#[test]
fn the_field_target_follows_focus_then_the_pointer() {
    let mut page = Page::built_in();
    let id = SceneEntityId(3);
    page.document.select(Some(id));
    page.idle();
    assert_eq!(page.panels.field_target(), None);

    let y = page.centre(page.axis_field(0, 1));
    page.frame(PointerInput::hovering(y), 0.0);
    assert_eq!(
        page.panels.field_target(),
        Some(&FieldTarget {
            entity: id,
            path: "position.1".to_owned(),
        }),
    );

    let row = page.centre(page.panels.row_keys()[4]);
    page.frame(PointerInput::hovering(row), 0.0);
    assert_eq!(
        page.panels.field_target(),
        None,
        "the outliner named a field"
    );

    let width = page.centre(page.axis_field(1, 0));
    page.click(width);
    page.frame(PointerInput::hovering(row), 0.0);
    assert_eq!(
        page.panels.field_target().map(|field| field.path.as_str()),
        Some("half_extents.0"),
        "the focused field was not the target with the pointer elsewhere",
    );

    page.click(row);
    assert_eq!(
        page.panels.field_target(),
        None,
        "a click on a row left the keyboard's target in the inspector",
    );
}
