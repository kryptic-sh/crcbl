//! The widgets that show and hide what is under them: a collapsing header and
//! a tree node.
//!
//! Each is a container holding a focusable row — a [`Behavior::BUTTON`], so a
//! click or accept toggles it — and, while it is open, the body its caller
//! builds. **A closed body is not built at all**, so it takes no layout, draws
//! nothing and costs nothing, and whatever state its own nodes kept is dropped
//! with them. Whether the row is open is kept in the store and survives every
//! rebuild of the row.
//!
//! A tree node's row also answers left and right while focused, following the
//! WAI-ARIA Authoring Practices' tree view pattern; `focus/mod.rs` has the
//! rule, and [`Ui::tree_item_step`] is it.

use std::panic::Location;

use super::{Ui, WidgetState};
use crate::style::PseudoClasses;
use crate::tree::{Behavior, Direction, NodeKey, Response};

/// Which of the rows this module builds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    /// A collapsing header's.
    Header,
    /// A tree node's, with children.
    Branch,
    /// A tree node's, without.
    Leaf,
}

impl Row {
    /// The row's selector, and its toggle's and its title's.
    const fn selectors(self) -> [&'static str; 3] {
        match self {
            Self::Header => [
                ".collapsing-header",
                ".collapsing-toggle",
                ".collapsing-title",
            ],
            Self::Branch | Self::Leaf => [".tree-row", ".tree-toggle", ".tree-label"],
        }
    }
}

impl Ui {
    /// A collapsing header: a `collapsing` block holding a
    /// `.collapsing-header` row — a `.collapsing-toggle` span showing `+` or
    /// `-`, then a `.collapsing-title` span — and, while open, a
    /// `.collapsing-body` block `body` builds. A click or accept on the row
    /// toggles it; it starts closed. Returns the row's [`Response`], with
    /// [`Response::changed`] set the frame it toggled; [`Ui::is_open`] takes
    /// its key.
    #[track_caller]
    pub fn collapsing(
        &mut self,
        selector: &str,
        title: &str,
        body: impl FnOnce(&mut Self),
    ) -> Response {
        let selector = super::typed("collapsing", selector);
        let mut row = None;
        self.block_with(&selector, &[], Behavior::NONE, |ui| {
            let (response, open) = ui.disclosure_row(Row::Header, title);
            row = Some(response);
            if open {
                ui.block(".collapsing-body", &[], body);
            }
        });
        row.expect("the row is built inside the block")
    }

    /// A tree node with children: a `tree-node` block holding a `.tree-row` —
    /// a `.tree-toggle` span showing `+` or `-`, then a `.tree-label` span —
    /// and, while open, a `.tree-children` block `children` builds, whose tree
    /// nodes are this one's children. A click or accept toggles it, and so do
    /// left and right as `focus/mod.rs` describes; it starts closed. Returns
    /// the row's [`Response`], as [`Ui::collapsing`] does.
    #[track_caller]
    pub fn tree_node(
        &mut self,
        selector: &str,
        label: &str,
        children: impl FnOnce(&mut Self),
    ) -> Response {
        let selector = super::typed("tree-node", selector);
        let mut row = None;
        self.block_with(&selector, &[], Behavior::NONE, |ui| {
            let (response, open) = ui.disclosure_row(Row::Branch, label);
            row = Some(response);
            if open {
                ui.tree_rows.push(response.key);
                ui.block(".tree-children", &[], children);
                ui.tree_rows.pop();
            }
        });
        row.expect("the row is built inside the block")
    }

    /// A tree node with no children: [`Ui::tree_node`]'s block and row, its
    /// toggle a space, which neither a click nor right opens.
    #[track_caller]
    pub fn tree_leaf(&mut self, selector: &str, label: &str) -> Response {
        let selector = super::typed("tree-node", selector);
        let mut row = None;
        self.block_with(&selector, &[], Behavior::NONE, |ui| {
            row = Some(ui.disclosure_row(Row::Leaf, label).0);
        });
        row.expect("the row is built inside the block")
    }

    /// The row of a header or a tree node, inside its open container: toggles
    /// it on a click or accept, builds it, keeps its state, and returns its
    /// response and whether it is open.
    fn disclosure_row(&mut self, kind: Row, title: &str) -> (Response, bool) {
        let [row, toggle, label] = kind.selectors();
        let parsed = self.node_selector(row);
        // One row per container, so this call site keys it.
        let key = self.widget_key(parsed, Location::caller());
        let shown = self
            .store
            .by_key(key)
            .is_some_and(|node| node.state.contains(PseudoClasses::OPEN));
        let mut open = self.is_open(key) && kind != Row::Leaf;
        if kind != Row::Leaf && self.interaction_of(key).clicked && !self.building_disabled() {
            open = !open;
        }
        let state = if open {
            PseudoClasses::OPEN
        } else {
            PseudoClasses::NONE
        };
        let glyph = match (kind, open) {
            (Row::Leaf, _) => " ",
            (_, true) => "-",
            (_, false) => "+",
        };
        let mut response = self.open_block(key, parsed, &[], Behavior::BUTTON, state, |ui| {
            ui.span(toggle, glyph, &[]);
            ui.span(label, title, &[]);
        });
        response.changed = open != shown;

        let state = match kind {
            Row::Header => WidgetState::Header { open },
            Row::Branch | Row::Leaf => {
                let parent = self.tree_rows.last().copied();
                if let Some(parent) = parent
                    && let WidgetState::TreeItem {
                        open,
                        branch,
                        parent: grandparent,
                        first_child: None,
                        item,
                    } = self.widget_state(parent)
                {
                    self.set_widget_state(
                        parent,
                        WidgetState::TreeItem {
                            open,
                            branch,
                            parent: grandparent,
                            first_child: Some(key),
                            item,
                        },
                    );
                }
                WidgetState::TreeItem {
                    open,
                    branch: kind == Row::Branch,
                    parent,
                    first_child: None,
                    // A tree node's open state is this node's own, not an
                    // outliner item's.
                    item: None,
                }
            }
        };
        self.set_widget_state(key, state);
        (response, open)
    }

    /// A left or right step on a focused tree row, as the WAI-ARIA tree view
    /// pattern answers it: right opens a closed node and moves to an open
    /// one's first child; left closes an open node and moves from a child to
    /// its parent. Returns false — and changes nothing — where the pattern
    /// does nothing, or the node it would move to cannot take focus, so that
    /// the step moves focus where the layout says instead.
    pub(in crate::tree) fn tree_item_step(&mut self, direction: Direction) -> bool {
        let Some(focused) = self.focused() else {
            return false;
        };
        let WidgetState::TreeItem {
            open,
            branch,
            parent,
            first_child,
            item,
        } = self.widget_state(focused)
        else {
            return false;
        };
        let reachable = |ui: &Self, key: Option<NodeKey>| {
            let modal = ui.active_modal();
            key.filter(|&key| ui.can_focus(key) && ui.inside(key, modal))
        };
        let toggled = |open| WidgetState::TreeItem {
            open,
            branch,
            parent,
            first_child,
            item,
        };
        match direction {
            Direction::Right if branch && !open => {
                self.set_widget_state(focused, toggled(true));
                self.tree_toggled = Some(focused);
                true
            }
            Direction::Right if branch => match reachable(self, first_child) {
                Some(child) => {
                    self.move_focus(child);
                    true
                }
                None => false,
            },
            Direction::Left if branch && open => {
                self.set_widget_state(focused, toggled(false));
                self.tree_toggled = Some(focused);
                true
            }
            Direction::Left => match reachable(self, parent) {
                Some(parent) => {
                    self.move_focus(parent);
                    true
                }
                None => false,
            },
            Direction::Up | Direction::Down | Direction::Right => false,
        }
    }
}
