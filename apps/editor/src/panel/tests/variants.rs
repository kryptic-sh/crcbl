//! A body's kind switched by clicking its variant strip in the inspector: the
//! panel offers the variants, and a pick lands in the log as one command.

use super::*;

use crcbl::scene_physics::{BODIES, Body, BodyKind};

use crate::document::physics_tests::{FALLING, falling};
use crate::scene::BLOCKS;

/// The node holding the span that reads `text` under `at`, searched depth
/// first: a header row for a collapsing title, an option for its label.
fn holder_of(ui: &Ui, at: NodeKey, text: &str) -> Option<NodeKey> {
    for child in ui.child_keys(at) {
        if ui.text(child) == Some(text) {
            return Some(at);
        }
        if let Some(found) = holder_of(ui, child, text) {
            return Some(found);
        }
    }
    None
}

/// The node holding `text` in the inspector's body section.
fn in_body_section(page: &Page, text: &str) -> NodeKey {
    let section = page
        .panels
        .section_fields(1)
        .expect("the inspector drew a body section");
    holder_of(page.panels.ui(), section, text)
        .unwrap_or_else(|| panic!("the body section shows no {text:?}"))
}

/// `FALLING`'s body kind.
fn kind(page: &mut Page) -> BodyKind {
    page.document
        .component(FALLING, BODIES)
        .expect("a body")
        .as_any()
        .downcast_ref::<Body>()
        .expect("the bodies system holds `Body`")
        .kind
}

/// **Clicking `Static` in a body's variant strip switches its kind as one
/// undoable entry**: the group opens on the strip, the click is one command,
/// the next frame's header names the new kind, and undo and redo walk it.
#[test]
fn clicking_a_bodys_kind_in_the_strip_switches_it_as_one_undo() {
    let mut page = Page::over(falling());
    page.document.select(Some(FALLING));
    page.idle();
    assert_eq!(page.panels.section_systems(), [BLOCKS, BODIES]);
    let saved = page.document.files().expect("the scene saves");

    let header = in_body_section(&page, "Kind: Dynamic");
    let at = page.centre(header);
    page.click(at);
    for variant in ["Dynamic", "Static", "Kinematic"] {
        in_body_section(&page, variant);
    }
    assert!(page.document.log().is_empty(), "opening the group edited");

    let option = in_body_section(&page, "Static");
    let at = page.centre(option);
    page.click(at);
    assert_eq!(kind(&mut page), BodyKind::Static);
    assert_eq!(page.document.log().len(), 1, "a pick is not one entry");
    in_body_section(&page, "Kind: Static");

    assert!(page.document.undo().expect("one entry"));
    assert_eq!(kind(&mut page), BodyKind::Dynamic);
    assert_eq!(page.document.files().expect("the scene saves"), saved);
    assert!(page.document.redo().expect("one entry above"));
    assert_eq!(kind(&mut page), BodyKind::Static);
}
