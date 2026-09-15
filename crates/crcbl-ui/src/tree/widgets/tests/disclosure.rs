//! The collapsing header and the tree node: toggling, what a closed body
//! costs, state across rebuilds, and the tree view pattern's arrow keys.

use super::*;
use crate::style::PseudoClasses;
use crate::tree::Response;

/// What one frame of the header page built.
struct HeaderPage {
    header: Response,
    body_built: bool,
    below: Response,
}

/// A collapsing header whose body is one 40-pixel block, over a button.
fn header_page(ui: &mut Ui, pointer: PointerInput, nav: NavInput) -> HeaderPage {
    frame(ui, pointer, nav, |ui| {
        let mut body_built = false;
        let header = ui.collapsing("#more", "More", |ui| {
            body_built = true;
            ui.block("", &[Declaration::Height(LengthAuto::Px(40.0))], |_| {});
        });
        let below = ui.button("#below", "Below");
        HeaderPage {
            header,
            body_built,
            below,
        }
    })
}

/// **A collapsing header toggles on a click and on accept, its body is not
/// built while it is closed — so what follows it moves up into the space —
/// and its state survives any number of rebuilds**, with `:open` on its row
/// only while open.
#[test]
fn a_collapsing_header_toggles_skips_its_closed_body_and_keeps_its_state() {
    let mut ui = Ui::new();
    let closed = header_page(&mut ui, idle(), NavInput::default());
    assert!(!closed.body_built, "a header starts open");
    let (_, header_bottom) = rect(&ui, closed.header.key);
    let below_closed = rect(&ui, closed.below.key).0.y;
    assert_eq!(
        below_closed, header_bottom.y,
        "a closed header's body took layout"
    );

    let on = centre(&ui, closed.header.key);
    header_page(&mut ui, press(on), NavInput::default());
    let opened = header_page(&mut ui, release(on), NavInput::default());
    assert!(
        opened.body_built && opened.header.changed,
        "a click did not open it"
    );
    assert!(ui.is_open(opened.header.key));
    let open_state = |ui: &Ui, key| {
        ui.store
            .by_key(key)
            .is_some_and(|node| node.state.contains(PseudoClasses::OPEN))
    };
    assert!(open_state(&ui, opened.header.key), "`:open` was not set");
    for _ in 0..5 {
        let page = header_page(&mut ui, idle(), NavInput::default());
        assert!(
            page.body_built && !page.header.changed,
            "a rebuild lost the state"
        );
    }
    let below_open = rect(&ui, opened.below.key).0.y;
    assert!(
        below_open >= below_closed + 40.0,
        "the open body did not push the button down: {below_closed} to {below_open}"
    );

    header_page(&mut ui, idle(), NavInput::NAVIGATION);
    let accepted = header_page(&mut ui, idle(), NavInput::ACCEPT);
    assert!(accepted.header.focused, "focus did not stay on the header");
    assert!(
        !accepted.body_built && accepted.header.changed,
        "accept did not close it"
    );
    header_page(&mut ui, idle(), NavInput::default());
    assert_eq!(rect(&ui, accepted.below.key).0.y, below_closed);
    assert!(!open_state(&ui, accepted.header.key), "`:open` stayed");
}

/// The rows of the tree page, by name.
struct TreePage {
    beside: Response,
    root: Response,
    child: Option<Response>,
    grandchild: Option<Response>,
    sibling: Option<Response>,
}

/// A button, and beside it a tree: `root` holds `child` — which holds the
/// leaf `grandchild` — and the leaf `sibling`.
fn tree_page(ui: &mut Ui, nav: NavInput) -> TreePage {
    frame(ui, idle(), nav, |ui| {
        let mut page = None;
        ui.block(
            "",
            &[Declaration::AlignItems(Some(crate::tree::Align::FlexStart))],
            |ui| {
                let beside = ui.button("#beside", "B");
                let (mut child, mut grandchild, mut sibling) = (None, None, None);
                ui.block(
                    "",
                    &[Declaration::FlexDirection(FlexDirection::Column)],
                    |ui| {
                        let root = ui.tree_node("#root", "root", |ui| {
                            child = Some(ui.tree_node("#child", "child", |ui| {
                                grandchild = Some(ui.tree_leaf("#grandchild", "grandchild"));
                            }));
                            sibling = Some(ui.tree_leaf("#sibling", "sibling"));
                        });
                        page = Some(TreePage {
                            beside,
                            root,
                            child,
                            grandchild,
                            sibling,
                        });
                    },
                );
            },
        );
        page.expect("built")
    })
}

/// **Left and right on a tree row follow the WAI-ARIA tree view pattern**:
/// right opens a closed node without moving, then moves to its first child;
/// left on a leaf moves to its parent, closes an open node, and moves from a
/// closed child to its parent — and where the pattern does nothing, the step
/// moves focus as the layout says, so left off a closed root reaches the
/// button beside the tree. Up and down move between the rows shown.
#[test]
fn tree_rows_answer_left_and_right_as_the_tree_view_pattern_does() {
    let mut ui = Ui::new();
    let first = tree_page(&mut ui, NavInput::default());
    ui.set_focus(first.root.key);
    let page = tree_page(&mut ui, NavInput::NAVIGATION);
    assert_eq!(ui.focused(), Some(page.root.key));
    assert!(page.child.is_none(), "a tree node starts open");

    let page = tree_page(&mut ui, RIGHT);
    assert_eq!(
        ui.focused(),
        Some(page.root.key),
        "opening the root moved focus"
    );
    let child = page.child.expect("right opened the root");
    assert!(page.root.changed);

    tree_page(&mut ui, RIGHT);
    assert_eq!(
        ui.focused(),
        Some(child.key),
        "right on an open node did not reach its first child"
    );
    tree_page(&mut ui, RIGHT);
    let page = tree_page(&mut ui, RIGHT);
    let grandchild = page.grandchild.expect("right opened the child");
    assert_eq!(ui.focused(), Some(grandchild.key));

    // Right on an end node does nothing in the pattern: nothing lies right of
    // it in the layout either, so focus stays.
    tree_page(&mut ui, RIGHT);
    assert_eq!(ui.focused(), Some(grandchild.key));

    let page = tree_page(&mut ui, DOWN);
    assert_eq!(
        ui.focused(),
        page.sibling.map(|row| row.key),
        "down did not reach the next row shown"
    );
    tree_page(&mut ui, UP);
    assert_eq!(ui.focused(), Some(grandchild.key));

    tree_page(&mut ui, LEFT);
    assert_eq!(
        ui.focused(),
        Some(child.key),
        "left on a leaf did not reach its parent"
    );
    let page = tree_page(&mut ui, LEFT);
    assert!(
        page.grandchild.is_none() && ui.focused() == Some(child.key),
        "left on an open node did not close it in place"
    );
    tree_page(&mut ui, LEFT);
    assert_eq!(
        ui.focused(),
        Some(page.root.key),
        "left on a closed child did not reach its parent"
    );
    let page = tree_page(&mut ui, LEFT);
    assert!(
        page.child.is_none(),
        "left on the open root did not close it"
    );
    tree_page(&mut ui, LEFT);
    assert_eq!(
        ui.focused(),
        Some(page.beside.key),
        "left on a closed root stopped dead instead of moving by the layout"
    );
}

/// **A tree node's state survives rebuilds, and a node inside a closed parent
/// is not built, so its state goes with it** — reopening the parent shows the
/// child closed.
#[test]
fn a_tree_nodes_state_survives_rebuilds_and_goes_with_a_closed_parent() {
    let mut ui = Ui::new();
    let first = tree_page(&mut ui, NavInput::default());
    ui.set_focus(first.root.key);
    tree_page(&mut ui, NavInput::NAVIGATION);
    tree_page(&mut ui, RIGHT);
    tree_page(&mut ui, RIGHT);
    tree_page(&mut ui, RIGHT);
    for _ in 0..4 {
        let page = tree_page(&mut ui, NavInput::default());
        assert!(page.grandchild.is_some(), "a rebuild closed the child");
    }
    let child = tree_page(&mut ui, NavInput::default()).child.expect("open");
    ui.set_focus(first.root.key);
    tree_page(&mut ui, NavInput::NAVIGATION);
    tree_page(&mut ui, LEFT);
    assert!(!ui.is_open(first.root.key));
    assert!(
        !ui.is_open(child.key),
        "a node that was not built kept state"
    );
    let page = tree_page(&mut ui, RIGHT);
    assert!(
        page.child.is_some() && page.grandchild.is_none(),
        "the child came back open"
    );
}
