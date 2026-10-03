//! Several selected, in the panels: the outliner's plain, Ctrl and Shift
//! clicks, the primary's mark, an entity leaving the selection, and the
//! inspector saying whose fields it shows.

use super::*;

use crcbl::ui::DrawCommand;

/// The window point in the middle of entity `id`'s outliner row, in the
/// built-in scene: one header, then a row per block in id order.
fn row_of(page: &Page, id: u32) -> Vec2 {
    let index = usize::try_from(id).expect("a small id") + 1;
    page.centre(page.panels.row_keys()[index])
}

/// **A plain click on a row selects it alone, a Ctrl click adds a row as the
/// primary or takes it out, and a Shift click takes the run from the anchor
/// to the row clicked — which becomes the primary, below the anchor or above
/// it.**
#[test]
fn outliner_clicks_replace_toggle_and_take_a_range() {
    let mut page = Page::built_in();
    page.idle();
    let ids = |values: &[u32]| {
        values
            .iter()
            .copied()
            .map(SceneEntityId)
            .collect::<Vec<_>>()
    };

    page.click_with(row_of(&page, 0), SelectMode::Replace);
    assert_eq!(page.document.selection(), ids(&[0]));

    page.click_with(row_of(&page, 2), SelectMode::Toggle);
    assert_eq!(page.document.selection(), ids(&[0, 2]));
    assert_eq!(page.document.primary(), Some(SceneEntityId(2)));

    // The anchor is 2, the row the Ctrl click acted on.
    page.click_with(row_of(&page, 3), SelectMode::Range);
    assert_eq!(page.document.selection(), ids(&[2, 3]), "a run downwards");

    page.click_with(row_of(&page, 1), SelectMode::Range);
    assert_eq!(
        page.document.selection(),
        ids(&[2, 1]),
        "a run upwards from the same anchor, the row clicked the primary",
    );

    page.click_with(row_of(&page, 1), SelectMode::Toggle);
    assert_eq!(page.document.selection(), ids(&[2]), "Ctrl took it out");
    assert_eq!(
        page.panels.selected_rows(),
        [entity_row(SceneEntityId(2))],
        "the outliner and the document disagree",
    );

    page.click_with(row_of(&page, 3), SelectMode::Replace);
    assert_eq!(page.document.selection(), ids(&[3]));

    // A run of several upwards: the rows join in row order, and the one
    // clicked — first among them — still ends up the primary.
    page.click_with(row_of(&page, 0), SelectMode::Range);
    assert_eq!(page.document.selection(), ids(&[3, 1, 2, 0]));
}

/// **The outliner shows the document's whole selection, drops an entity a
/// delete takes out of it, and keeps one whose system is collapsed.**
#[test]
fn the_outliner_follows_the_selection_down_to_a_delete() {
    let mut page = Page::built_in();
    page.document
        .set_selection([SceneEntityId(1), SceneEntityId(2)]);
    page.idle();
    let mut rows = page.panels.selected_rows();
    rows.sort();
    assert_eq!(rows, [1, 2].map(|id| entity_row(SceneEntityId(id))));

    page.document.delete(&[SceneEntityId(2)]).expect("held");
    page.idle();
    assert_eq!(page.document.selection(), [SceneEntityId(1)]);
    assert_eq!(page.panels.selected_rows(), [entity_row(SceneEntityId(1))]);

    // Collapsing the system hides the row, and keeps the entity selected.
    page.panels.outliner.set_expanded(system_row(0), false);
    page.idle();
    assert!(
        page.panels.selected_rows().is_empty(),
        "the row still shows"
    );
    assert_eq!(
        page.document.selection(),
        [SceneEntityId(1)],
        "collapsing its system deselected it",
    );
}

/// The colour the outliner drew `label` in, read off the draw list.
fn label_color(page: &Page, label: &str) -> [f32; 4] {
    page.panels
        .draw_list()
        .commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::Text { text, color, .. } if text == label => Some(*color),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the outliner drew no {label:?}"))
}

/// **Every selected row is marked, and the primary's label in a colour of
/// its own** — so with two selected a person can tell which one the
/// inspector shows.
#[test]
fn the_primarys_row_reads_differently_from_the_other_selected_rows() {
    let mut page = Page::built_in();
    page.document
        .set_selection([SceneEntityId(1), SceneEntityId(2)]);
    page.idle();
    let label = |id: u32| {
        label_of(
            &page.panels.outline,
            &page.panels.names,
            entity_row(SceneEntityId(id)),
        )
    };
    let (primary, other, unselected) = (label(2), label(1), label(3));
    assert_ne!(
        label_color(&page, &primary),
        label_color(&page, &other),
        "the primary reads like any selected row",
    );
    assert_eq!(
        label_color(&page, &other),
        label_color(&page, &unselected),
        "a selected row that is not the primary is marked by its label",
    );
}

/// **The inspector edits the primary alone, and its title says so** with
/// several selected; with one, the title is that entity's label alone.
#[test]
fn the_inspector_shows_the_primary_and_says_so() {
    let mut page = Page::built_in();
    page.document
        .set_selection([SceneEntityId(1), SceneEntityId(3)]);
    page.idle();
    let drew = |page: &Page, wanted: &str| {
        page.panels
            .draw_list()
            .commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::Text { text, .. } if text == wanted))
    };
    assert!(
        drew(&page, "#3 (primary of 2 selected)"),
        "the title does not say it shows the primary"
    );
    let read = |page: &mut Page, id: u32| {
        page.document
            .read(SceneEntityId(id), crate::scene::BLOCKS, "position.0")
            .expect("a block's x")
    };
    let other = read(&mut page, 1);
    let was = read(&mut page, 3);
    let at = page.centre(page.axis_field(0, 0));
    page.drag_in_steps(at, Vec2::new(30.0, 0.0), 6);
    assert_ne!(read(&mut page, 3), was, "the drag did not edit the primary");
    assert_eq!(
        read(&mut page, 1),
        other,
        "the drag edited another selected"
    );
    assert_eq!(
        page.document.selection(),
        [SceneEntityId(1), SceneEntityId(3)],
        "an inspector edit changed the selection",
    );

    page.document.select(Some(SceneEntityId(3)));
    page.idle();
    assert!(
        drew(&page, "#3"),
        "one selected is titled by its label alone"
    );
}
