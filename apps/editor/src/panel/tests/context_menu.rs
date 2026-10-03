//! An outliner row's context menu, in the panels: what a right-click selects,
//! what a pick hands back, and the items play mode disables.

use super::*;

/// The outliner row of entity `id` in the built-in scene: one header, then a
/// row per block in id order.
fn row_key(page: &Page, id: u32) -> NodeKey {
    let index = usize::try_from(id).expect("a small id") + 1;
    page.panels.row_keys()[index]
}

/// A secondary press at the middle of `id`'s row.
fn right_click(page: &mut Page, id: u32) -> PanelFrame {
    let at = page.centre(row_key(page, id));
    page.frame(
        PointerInput {
            secondary_pressed: true,
            ..PointerInput::hovering(at)
        },
        0.0,
    )
}

/// The item of `id`'s open menu whose label starts with `name`.
fn item(page: &Page, id: u32, name: &str) -> NodeKey {
    let ui = page.panels.ui();
    ui.child_keys(Ui::popup_key(row_key(page, id)))
        .into_iter()
        .find(|&item| {
            ui.child_keys(item)
                .first()
                .and_then(|&label| ui.text(label))
                .is_some_and(|text| text.starts_with(name))
        })
        .unwrap_or_else(|| panic!("the menu holds no {name}"))
}

/// A click on `key` — press and release — answering what the release frame
/// handed back, which is the frame the tree resolves the click in.
fn click_reporting(page: &mut Page, key: NodeKey) -> PanelFrame {
    let at = page.centre(key);
    page.frame(
        PointerInput {
            pos: at,
            down: true,
            released: false,
            secondary_pressed: false,
        },
        0.0,
    );
    page.frame(
        PointerInput {
            pos: at,
            down: false,
            released: true,
            secondary_pressed: false,
        },
        0.0,
    )
}

/// **A right-click on a row outside the selection selects it alone, and its
/// menu's Delete hands back the action Delete's key asks for** — once, and
/// closing the menu.
#[test]
fn a_rows_menu_selects_the_row_and_hands_back_the_keys_action() {
    let mut page = Page::built_in();
    page.idle();
    page.document.select(Some(SceneEntityId(0)));
    page.idle();
    let opened = right_click(&mut page, 2);
    assert_eq!(opened.menu, None, "opening picked something");
    assert_eq!(page.document.selection(), [SceneEntityId(2)]);
    assert!(page.panels.ui().is_popup_open(row_key(&page, 2)));

    let delete = item(&page, 2, "Delete");
    let picked = click_reporting(&mut page, delete);
    assert_eq!(picked.menu, Some(Action::Delete));
    assert!(!page.panels.ui().is_popup_open(row_key(&page, 2)));
    assert_eq!(page.idle().menu, None, "the pick was handed back twice");
}

/// **A right-click on a row inside a multi-selection keeps the selection
/// whole**, so the menu acts on every selected entity.
#[test]
fn a_rows_menu_inside_a_multi_selection_keeps_it() {
    let mut page = Page::built_in();
    page.document
        .set_selection([SceneEntityId(1), SceneEntityId(3)]);
    page.idle();
    right_click(&mut page, 1);
    assert_eq!(
        page.document.selection(),
        [SceneEntityId(1), SceneEntityId(3)],
        "the right-click replaced the selection",
    );
    let duplicate = item(&page, 1, "Duplicate");
    assert_eq!(
        click_reporting(&mut page, duplicate).menu,
        Some(Action::Duplicate)
    );
}

/// **While a scene plays the menu's items are disabled**: a click on Delete
/// hands nothing back and leaves the menu open.
#[test]
fn a_rows_menu_is_disabled_in_play_mode() {
    let mut page = Page::built_in();
    page.idle();
    page.document.play().expect("the greybox scene plays");
    page.idle();
    right_click(&mut page, 2);
    assert!(page.panels.ui().is_popup_open(row_key(&page, 2)));
    for name in ["Rename", "Duplicate", "Delete"] {
        let key = item(&page, 2, name);
        assert_eq!(
            click_reporting(&mut page, key).menu,
            None,
            "{name} was picked in play mode"
        );
    }
    assert!(
        page.panels.ui().is_popup_open(row_key(&page, 2)),
        "a click on a disabled item closed the menu"
    );
}
