//! An entity in several systems: one outliner row for it, an inspector
//! section per system, and the buttons that attach and detach them.

use super::*;

use crate::command::EditCommand;
use crate::document::systems_tests::{SUN, two_systems};
use crate::scene::BLOCKS;

/// The first step: a block and a sun.
const BOTH: SceneEntityId = SceneEntityId(1);
/// A block alone.
const BLOCK: SceneEntityId = SceneEntityId(2);

/// **An entity in two systems is one outliner row**: a header per system and a
/// row per entity, every row naming a different entity.
#[test]
fn the_outliner_lists_an_entity_in_two_systems_once() {
    let mut page = Page::over(two_systems());
    page.idle();
    let entities = page.document.entity_count();
    let headers = page.document.outline().len();
    assert_eq!(
        page.panels.row_keys().len(),
        headers + entities,
        "an entity in two systems has a row under each",
    );

    // Clicking every row selects every entity once.
    let mut selected = Vec::new();
    for row in page.panels.row_keys() {
        let at = page.centre(row);
        page.click(at);
        selected.extend(page.document.selected());
    }
    selected.sort();
    selected.dedup();
    assert_eq!(selected.len(), entities, "{selected:?}");
}

/// **The inspector draws a section per system holding the selection**, in
/// manifest order, each with its remove button; an entity in one system has a
/// section and no remove button, and an add button per system it could join.
#[test]
fn the_inspector_draws_a_section_per_system() {
    let mut page = Page::over(two_systems());
    page.document.select(Some(BOTH));
    page.idle();
    assert_eq!(page.panels.section_systems(), [BLOCKS, SUN]);
    assert!(page.panels.remove_button(BLOCKS).is_some());
    assert!(page.panels.remove_button(SUN).is_some());
    let listed = [BLOCKS.to_owned(), SUN.to_owned()];
    assert!(
        page.panels
            .add_buttons()
            .iter()
            .all(|(system, _)| !listed.contains(system)),
        "1 is in every system the manifest lists",
    );
    // Each section's rows are its own component's: the sun has its four
    // fields, and the block its two vector rows and its rotation.
    let ui = page.panels.ui();
    let block_rows = ui.child_keys(page.panels.section_fields(0).expect("a block section"));
    let sun_rows = ui.child_keys(page.panels.section_fields(1).expect("a sun section"));
    assert_eq!(block_rows.len(), 3);
    assert_eq!(sun_rows.len(), 4);

    page.document.select(Some(BLOCK));
    page.idle();
    assert_eq!(page.panels.section_systems(), [BLOCKS]);
    assert_eq!(
        page.panels.remove_button(BLOCKS),
        None,
        "the last section offers a remove the document refuses",
    );
    let adds: Vec<String> = page
        .panels
        .add_buttons()
        .into_iter()
        .map(|(system, _)| system)
        .collect();
    assert_eq!(adds, page.document.attachable(BLOCK));
    assert_eq!(adds[0], SUN, "the manifest's systems come first");
}

/// **The add button attaches and a remove button detaches**, each one undoable
/// command, and the sections follow.
#[test]
fn the_add_and_remove_buttons_attach_and_detach() {
    let mut page = Page::over(two_systems());
    page.document.select(Some(BLOCK));
    page.idle();
    let (_, add) = page.panels.add_buttons()[0].clone();
    let at = page.centre(add);
    page.click(at);
    assert_eq!(page.document.systems_of(BLOCK), [BLOCKS, SUN]);
    assert_eq!(page.panels.section_systems(), [BLOCKS, SUN]);
    assert_eq!(page.document.log().len(), 1);

    let remove = page.panels.remove_button(BLOCKS).expect("two sections");
    let at = page.centre(remove);
    page.click(at);
    assert_eq!(page.document.systems_of(BLOCK), [SUN]);
    assert_eq!(page.panels.section_systems(), [SUN]);
    assert_eq!(page.document.log().len(), 2);

    assert!(page.document.undo().expect("the detach undoes"));
    assert!(page.document.undo().expect("the attach undoes"));
    page.idle();
    assert_eq!(page.panels.section_systems(), [BLOCKS]);
}

/// **A field in the second section is that system's**: the clipboard target
/// under the pointer names the sun, and an edit there lands in the sun.
#[test]
fn a_field_in_a_section_belongs_to_that_sections_system() {
    let mut page = Page::over(two_systems());
    page.document.select(Some(BOTH));
    page.idle();
    let ui = page.panels.ui();
    let sun_rows = ui.child_keys(page.panels.section_fields(1).expect("a sun section"));
    // A scalar row is a label span and then its widget.
    let widget = ui.child_keys(sun_rows[0])[1];
    let at = page.centre(widget);
    page.frame(PointerInput::hovering(at), 0.0);
    let target = page
        .panels
        .field_target()
        .expect("a field under the pointer")
        .clone();
    assert_eq!(target.system, SUN);
    assert_eq!(target.entity, BOTH);

    let before = page
        .document
        .read(BOTH, SUN, &target.path)
        .expect("a sun leaf");
    page.drag(at, Vec2::new(40.0, 0.0));
    let after = page
        .document
        .read(BOTH, SUN, &target.path)
        .expect("a sun leaf");
    assert_ne!(after, before, "the drag did not reach the sun");
    let EditCommand::SetProperty { system, .. } = page
        .document
        .log()
        .applied()
        .last()
        .expect("the drag was recorded")
        .clone()
    else {
        panic!("a field drag is a property set");
    };
    assert_eq!(system, SUN);
}
