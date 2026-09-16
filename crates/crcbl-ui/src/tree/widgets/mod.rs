//! The widget set on the tree: `docs/plan/07-ui-debug.md` rung 7.
//!
//! Every widget is a builder on [`Ui`] that composes blocks and spans, as the
//! plan says a widget is, and returns the [`Response`] of the node focus rests
//! on with [`Response::changed`] set when the widget changed what it edits.
//! They live here, inside `tree`, rather than in [`crate::widget`] because they
//! read what only the tree has — last frame's rectangles, the interaction the
//! frame began with, the engaged snapshot — and because `crate::widget` is the
//! pre-CSS toolkit this set replaces. One file per kind of behaviour:
//!
//! | file | builders | focus |
//! |---|---|---|
//! | `button.rs` | [`Ui::button`], [`Ui::checkbox`] | instant activation |
//! | `value.rs` | [`Ui::slider`], [`Ui::drag_value`] | engaged |
//! | `disclosure.rs` | [`Ui::collapsing`], [`Ui::tree_node`], [`Ui::tree_leaf`] | instant activation |
//! | `split.rs` | [`Ui::split`], [`Ui::split_at`] | the divider is engaged |
//! | `list.rs` | [`Ui::list`] | each row is instant activation |
//! | `text_input.rs` | [`Ui::text_input`], [`Ui::text_input_with`] | engaged |
//! | `outliner.rs` | [`Ui::outliner`], [`Ui::outliner_with`] | each row is instant activation |
//! | `tabs.rs` | [`Ui::tabs`] | each tab is instant activation |
//! | `dock.rs` | [`Ui::dock`] | each divider is engaged |
//!
//! # Values are the caller's
//!
//! A widget that edits a value takes it as `&mut` and writes it back the frame
//! the edit happens. What a widget keeps for itself in the store is
//! interaction state only: whether a header is open, where a split's divider
//! was dragged to, the value a drag started from, which row of a list holds
//! focus. It survives a rebuild as hover does, and goes when a frame does not
//! build the node — so the state of a tree node inside a collapsed parent,
//! which is not built, is dropped.
//!
//! # Selectors
//!
//! A widget's `selector` is its `#id.class` part: the widget's type — `button`,
//! `checkbox`, `slider`, `drag-value`, `collapsing`, `tree-node`, `split`,
//! `list`, `text-input`, `outliner`, `tabs` or `dock` — is put in front of it,
//! and that type is what `default.css` styles.
//! A selector that names a type of its own keeps it, which opts the widget out
//! of every engine rule for its type. The parts inside a widget have classes
//! named after it (`.slider-fill`, `.tree-row`); each builder's docs name them.
//!
//! # State the stylesheet sees
//!
//! Beside `:hover`, `:active`, `:focus`, `:engaged` and `:disabled`, a checked
//! checkbox has **`:checked`**, an open header, tree row or outliner row has
//! **`:open`**, and a text input the clipboard refused has **`:refused`**. A
//! selected outliner row and the tab that is showing have **`:checked`** too:
//! Selectors Level 4 §12.2 gives `:checked` to an `option` element that is
//! selected, which is what both are, and the eight pseudo-classes fill
//! [`PseudoClasses`](crate::style::PseudoClasses)' bits.
//! Both are pseudo-classes rather than classes, as Selectors Level 4 defines
//! them (§12.2's input value states and §11.1's collapse state): the state
//! belongs to the element and the user changes it, where a class is the
//! author's. A pseudo-class is also what the cascade's dependency tracking
//! sees, so toggling a checkbox re-resolves only nodes a `:checked` rule
//! tests.
//!
//! # Engage widgets
//!
//! A slider, a drag-value and a split's divider follow the LOCKED rule in
//! `focus/mod.rs`: focus moves past them until accept or a click engages one;
//! then left and right (up and down for a column split) adjust it, accept or a
//! click elsewhere commits, and back cancels to the value it had when it
//! engaged. A text input is engaged too; `text_input.rs` has what it takes
//! while it is. The pointer adjusts without engaging: a press drags the value, and
//! a press that moved past [`super::DRAG_THRESHOLD`] ends focused, not engaged.
//!
//! # Disabled
//!
//! [`Ui::enabled`] with `false` disables every node built inside it: none is
//! focused, clicked or engaged, and no widget in it moves its value.

mod button;
mod disclosure;
mod dock;
mod list;
mod outliner;
mod split;
mod tabs;
#[cfg(test)]
mod tests;
mod text_input;
mod value;

use std::borrow::Cow;
use std::panic::Location;

#[cfg(doc)]
use super::Response;
use super::Ui;
use super::store::{Interaction, NodeKey};
use super::style::LengthAuto;
use crate::style::NodeSelector;

pub use dock::{DockLayout, DockSide};
pub use list::LIST_OVERSCAN;
pub use outliner::{
    OUTLINER_INDENT, OUTLINER_ROW_HEIGHT, OutlinerBuilder, OutlinerId, OutlinerOptions,
    OutlinerRow, OutlinerState, SelectMode,
};
pub use split::{SPLIT_NAV_STEP, SplitAxis};
pub use text_input::{
    ClipboardAnswer, ClipboardReply, ClipboardRequest, DOUBLE_CLICK_TIME, MASK, TextInput,
    TextInputOptions,
};
pub(crate) use text_input::{EditState, TextFit};

/// What a widget keeps on one node between frames; see the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum WidgetState {
    /// Nothing: a plain node.
    #[default]
    None,
    /// A collapsing header's row: whether it is open.
    Header {
        /// Whether its body is built.
        open: bool,
    },
    /// A tree node's or an outliner's row.
    TreeItem {
        /// Whether its children are built.
        open: bool,
        /// Whether it has children to open: false for a leaf.
        branch: bool,
        /// The row of the tree node it was built inside.
        parent: Option<NodeKey>,
        /// The row of the first tree node built inside it, last frame.
        first_child: Option<NodeKey>,
        /// The item an outliner row stands for; none for a [`Ui::tree_node`],
        /// whose open state is this node's own.
        item: Option<OutlinerId>,
    },
    /// A drag-value or a divider under a press: what the value was when the
    /// press began.
    Anchor(Option<f32>),
    /// A split: where its divider was put, as the first pane's length.
    Split(Option<f32>),
    /// A virtualized list or outliner: the row that held focus when it was
    /// last built.
    Rows(Option<usize>),
    /// An outliner row's toggle: the item a click on it expands or collapses.
    OutlinerToggle(OutlinerId),
    /// A tab strip: the tab that is showing, as the hash of its title.
    Tabs(u64),
    /// A text input, whose editing state is [`Ui`]'s `edits`: a drag that
    /// ends on it leaves it engaged.
    TextInput,
}

/// `selector` with the widget type `kind` in front, unless it names a type.
fn typed<'s>(kind: &'static str, selector: &'s str) -> Cow<'s, str> {
    if selector.is_empty() {
        Cow::Borrowed(kind)
    } else if selector.starts_with(['#', '.']) {
        Cow::Owned(format!("{kind}{selector}"))
    } else {
        Cow::Borrowed(selector)
    }
}

/// `value` held inside `min..=max` without panicking: a NaN is `min`, and a
/// range whose start is past its end holds every value at its end.
fn clamp_to(value: f32, min: f32, max: f32) -> f32 {
    value.max(min).min(max)
}

impl Ui {
    /// The key the next node built from `selector` at `location` gets, after
    /// the duplicate rule — so a widget can read the node's stored state before
    /// it builds it.
    fn widget_key(&mut self, selector: NodeSelector<'_>, location: &Location<'_>) -> NodeKey {
        let key = self.selector_key(selector, location);
        self.unique(key)
    }

    /// The interaction `key` began this frame with; none for a node not yet
    /// stored.
    fn interaction_of(&self, key: NodeKey) -> Interaction {
        self.store
            .by_key(key)
            .map(|node| node.interaction)
            .unwrap_or_default()
    }

    /// What the widget on `key` kept.
    fn widget_state(&self, key: NodeKey) -> WidgetState {
        self.store
            .by_key(key)
            .map_or(WidgetState::None, |node| node.widget)
    }

    /// Keeps `state` on `key`, which this frame built.
    fn set_widget_state(&mut self, key: NodeKey, state: WidgetState) {
        if let Some(slot) = self.store.find(key) {
            self.store.get_mut(slot).widget = state;
        }
    }

    /// Whether a widget built now is disabled.
    const fn building_disabled(&self) -> bool {
        self.disabled_depth > 0
    }

    /// Builds `build` with every node in it disabled when `enabled` is false:
    /// `:disabled`, never focused, clicked or engaged, and no widget in it
    /// moves its value. With `true` it only builds.
    pub fn enabled(&mut self, enabled: bool, build: impl FnOnce(&mut Self)) {
        let disables = u32::from(!enabled);
        self.disabled_depth += disables;
        build(self);
        self.disabled_depth -= disables;
    }

    /// Whether the collapsing header, tree node or outliner row whose row is
    /// `key` is open. False for any other node.
    #[must_use]
    pub fn is_open(&self, key: NodeKey) -> bool {
        matches!(
            self.widget_state(key),
            WidgetState::Header { open: true } | WidgetState::TreeItem { open: true, .. }
        )
    }

    /// The node this frame's pointer clicked, or none.
    const fn clicked_key(&self) -> Option<NodeKey> {
        self.clicked
    }

    /// Which of `rows` rows of `height` pixels the innermost open block builds
    /// this frame: the rows its view shows at its scroll offset and
    /// [`LIST_OVERSCAN`] more on each side. Both [`Ui::list`] and
    /// [`Ui::outliner`] window their rows with it, so the two agree by
    /// construction on what "in the window" means.
    ///
    /// Panics outside every block.
    fn row_window(&self, rows: usize, height: f32) -> RowWindow {
        let block = &self.nodes[*self.open.last().expect("called inside the block")];
        let stored = self.store.get(block.slot);
        let view = if block.fresh {
            // Nothing laid out yet: the stylesheet's height, when it is one.
            match block.style.height {
                LengthAuto::Px(px) => px,
                LengthAuto::Percent(_) | LengthAuto::Auto => 0.0,
            }
        } else {
            let (start, end) = stored.content_box();
            end.y - start.y
        };
        let offset = stored.scroll_offset.y.max(0.0);

        // Float-to-integer casts saturate, so a huge offset is the last row.
        let first = ((offset / height).floor() as usize).saturating_sub(LIST_OVERSCAN);
        let last = ((offset + view.max(0.0)) / height).ceil() as usize;
        let end = last.saturating_add(LIST_OVERSCAN).min(rows);
        RowWindow {
            first: first.min(end),
            end,
        }
    }

    /// The row a virtualized block kept focus on, when it is still a row and
    /// still the focused node. `key` is the block's, and `row_key` makes a
    /// row's key from its index.
    fn kept_row(
        &self,
        key: NodeKey,
        rows: usize,
        row_key: impl Fn(&Self, usize) -> NodeKey,
    ) -> Option<usize> {
        let kept = match self.widget_state(key) {
            WidgetState::Rows(focused) => focused.filter(|&index| index < rows),
            _ => None,
        }?;
        let expected = row_key(self, kept);
        (self.focused() == Some(expected)).then_some(kept)
    }
}

/// Which rows a virtualized block builds this frame; see [`Ui::row_window`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RowWindow {
    /// The first row in the window.
    pub first: usize,
    /// One past the last row in the window.
    pub end: usize,
}
