//! The widget set on the tree: UI rung 7.
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
//! | `number_entry.rs` | a drag-value being typed into | engaged |
//! | `disclosure.rs` | [`Ui::collapsing`], [`Ui::tree_node`], [`Ui::tree_leaf`] | instant activation |
//! | `split.rs` | [`Ui::split`], [`Ui::split_at`] | the divider is engaged |
//! | `list.rs` | [`Ui::list`] | each row is instant activation |
//! | `text_input.rs` | [`Ui::text_input`], [`Ui::text_input_with`] | engaged |
//! | `outliner.rs` | [`Ui::outliner`], [`Ui::outliner_with`] | each row is instant activation |
//! | `tabs.rs` | [`Ui::tabs`] | each tab is instant activation |
//! | `select.rs` | [`Ui::select`] | the button and each option are instant activation |
//! | `context_menu.rs` | [`Ui::context_menu`] | each item is instant activation |
//! | `dock.rs` | [`Ui::dock`] | each divider is engaged |
//! | `inspector.rs` | [`Ui::inspector`], [`Ui::inspector_with`] | each row's own widget |
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
//! `list`, `text-input`, `outliner`, `tabs`, `select`, `dock` or `inspector` —
//! is put in front of it, and that type is what `default.css` styles. A
//! selector that names a type of its own keeps it, which opts the widget out
//! of every engine rule for its type. The parts inside a widget have classes
//! named after it (`.slider-fill`, `.tree-row`); each builder's docs name them.
//! A pop-up's root has the type `popup` (`popup.rs`), and a context menu's is
//! a `popup.context-menu`.
//!
//! # State the stylesheet sees
//!
//! Beside `:hover`, `:active`, `:focus`, `:engaged` and `:disabled`, a checked
//! checkbox has **`:checked`**, an open header, tree row or outliner row has
//! **`:open`**, and a text input the clipboard refused, or a drag-value that
//! refused the number typed into it, has **`:refused`**. A selected outliner row and the tab that is showing have **`:checked`** too:
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
//! while it is, and a drag-value opened for typing takes the same
//! (`number_entry.rs`). The pointer adjusts without engaging: a press drags the value, and
//! a press that moved past [`super::DRAG_THRESHOLD`] ends focused, not engaged.
//!
//! # Disabled
//!
//! [`Ui::enabled`] with `false` disables every node built inside it: none is
//! focused, clicked or engaged, and no widget in it moves its value.

mod button;
mod context_menu;
mod disclosure;
mod dock;
mod inspector;
mod list;
mod number_entry;
mod outliner;
mod scrollbar;
mod select;
mod split;
mod tabs;
#[cfg(test)]
mod tests;
mod text_input;
mod value;

use std::borrow::Cow;
use std::panic::Location;
use std::time::Duration;

use glam::Vec2;

#[cfg(doc)]
use super::Response;
use super::Ui;
use super::store::{Interaction, NodeKey};
use super::style::LengthAuto;
use crate::style::NodeSelector;

pub use context_menu::{CONTEXT_ARROW, ContextItem, ContextMenuResponse};
pub use dock::{DockLayout, DockSide};
pub use inspector::{
    AXES, FieldEdit, FieldRow, INSPECTOR_STEP, Inspection, InspectorOptions, Overrides, RowBuilder,
    VARIANT_LABEL, VariantEdit, WHOLE_STEP,
};
pub use list::LIST_OVERSCAN;
pub use outliner::{
    OUTLINER_INDENT, OUTLINER_ROW_HEIGHT, OutlinerBuilder, OutlinerId, OutlinerOptions,
    OutlinerRow, OutlinerState, SelectMode,
};
pub use scrollbar::MIN_SCROLL_HANDLE;
pub use select::SELECT_CARET;
pub use split::{SPLIT_NAV_STEP, SplitAxis};
pub use text_input::{
    ClipboardAnswer, ClipboardReply, ClipboardRequest, DOUBLE_CLICK_TIME, MASK, Preedit, TextInput,
    TextInputOptions,
};
pub(crate) use text_input::{EditState, TextFit};
pub use value::DragNumber;

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
    /// A divider under a press: where it was when the press began.
    Anchor(Option<f32>),
    /// A drag-value that is not being typed into.
    Drag {
        /// What its value was when the press it is under began, in the
        /// number's own kind; none while no press holds it.
        anchor: Option<value::sealed::DragAnchor>,
        /// When on the text clock, and where, its last press that was not a
        /// drag began: what a second press is a double-click after.
        last_press: Option<(Duration, Vec2)>,
    },
    /// A drag-value being typed into, whose line is [`Ui`]'s `edits` as a
    /// text input's is: the value it had when it engaged, which back puts
    /// back.
    TypedNumber(value::sealed::DragAnchor),
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
    /// A context menu's item that opens a submenu beside it, which the right
    /// arrow opens too.
    MenuBranch,
}

impl WidgetState {
    /// Whether the node takes typed text while it is engaged — a text input,
    /// or a drag-value being typed into — so a drag that ends on it leaves it
    /// engaged, and [`Ui::text_editing`] is true.
    pub(crate) const fn types(self) -> bool {
        matches!(self, Self::TextInput | Self::TypedNumber(_))
    }

    /// Whether the node is a drag-value being typed into, which Tab and
    /// Shift+Tab put the number in and leave.
    pub(crate) const fn is_typed_number(self) -> bool {
        matches!(self, Self::TypedNumber(_))
    }

    /// Whether the node is a drag-value, typed into or not: what Tab out of a
    /// number being typed opens for typing when it lands there.
    pub(crate) const fn is_drag_value(self) -> bool {
        matches!(self, Self::Drag { .. } | Self::TypedNumber(_))
    }
}

/// What a widget builder built a node as, recorded by the builder itself for
/// the frame: the role an accessibility tree gives the node. It cannot be read
/// off the selector, because a caller-named type replaces the one [`typed`]
/// puts there. Cleared each time the node is built, so a node no builder
/// marked this frame is [`WidgetKind::None`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WidgetKind {
    /// A plain block or span: no widget built it as anything.
    #[default]
    None,
    /// Drawn for looks alone — a checkbox's box, a toggle's glyph, a caret,
    /// a separator — and left out of the accessibility tree with everything
    /// inside it.
    Decoration,
    /// [`Ui::button`].
    Button,
    /// [`Ui::checkbox`]'s block, `:checked` while its value is true.
    Checkbox,
    /// [`Ui::slider`].
    Slider,
    /// [`Ui::drag_value`], whether or not it is being typed into.
    DragValue,
    /// [`Ui::text_input_with`]'s block.
    TextInput {
        /// Whether it draws its text masked, as a password field does.
        masked: bool,
    },
    /// [`Ui::select`]'s button, `:open` while its list is.
    Select,
    /// The pop-up holding a [`Ui::select`]'s options.
    SelectList,
    /// One option of a [`Ui::select`], `:checked` while it is the chosen one.
    SelectOption,
    /// A [`Ui::collapsing`] header's row, `:open` while its body is built.
    Disclosure,
    /// The row of a [`Ui::tree_node`] or a [`Ui::tree_leaf`].
    TreeRow,
    /// [`Ui::outliner`]'s block.
    Outliner,
    /// One row of an outliner, which holds what the caller built in it.
    OutlinerRow,
    /// A branch row's toggle in an outliner, which a click opens or closes.
    OutlinerToggle,
    /// [`Ui::list`]'s block.
    List,
    /// One row of a list, which holds what the caller built in it.
    ListRow,
    /// The strip of a [`Ui::tabs`].
    TabStrip,
    /// One tab, `:checked` while it is the one showing.
    Tab,
    /// The pane showing the chosen tab.
    TabPanel,
    /// The divider of a split or a dock.
    Splitter,
    /// One pane of a [`Ui::dock`].
    Pane,
    /// [`Ui::scrollbar`]'s track.
    ScrollBar,
    /// A pop-up's root, unless the widget that opened it says more.
    Popup,
    /// A context menu's pop-up.
    Menu,
    /// A context menu's item, `:open` while a submenu it opens is.
    MenuItem,
    /// A tooltip's root.
    Tooltip,
    /// [`Ui::inspector_with`]'s block.
    Inspector,
}

/// `selector` with the widget type `kind` in front, unless it names a type.
pub(super) fn typed<'s>(kind: &'static str, selector: &'s str) -> Cow<'s, str> {
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

    /// Records that this frame's builder built `key` as `kind`.
    pub(in crate::tree) fn mark(&mut self, key: NodeKey, kind: WidgetKind) {
        if let Some(slot) = self.store.find(key) {
            self.store.get_mut(slot).kind = kind;
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
    /// The window is this frame's: `rows` is this frame's count, and the
    /// offset is clamped to it as layout will clamp it, so a list that shrank
    /// under a deep offset still builds the rows it now ends with. The view is
    /// the height the block was last laid out with, which only layout can
    /// know; a block never laid out takes its stylesheet's height when that
    /// is a length, else the last layout's viewport, past which no row can
    /// show. The window is recorded for [`Ui::needs_rebuild`], which reports a
    /// view that layout then found showing rows the window left out.
    ///
    /// Panics outside every block.
    fn row_window(&mut self, rows: usize, height: f32) -> RowWindow {
        let index = *self.open.last().expect("called inside the block");
        let block = &self.nodes[index];
        let stored = self.store.get(block.slot);
        let view = if block.fresh {
            match block.style.height {
                LengthAuto::Px(px) => px,
                LengthAuto::Percent(_) | LengthAuto::Auto => self
                    .viewport
                    .map(|viewport| viewport.max.y - viewport.min.y)
                    .filter(|height| height.is_finite())
                    .unwrap_or(0.0),
            }
        } else {
            let (start, end) = stored.content_box();
            end.y - start.y
        };
        let reach = (rows as f32 * height - view).max(0.0);
        let offset = stored.scroll_offset.y.max(0.0).min(reach);
        let window = window_of(rows, height, offset, view, LIST_OVERSCAN);
        self.built_rows.push(BuiltRows {
            node: index,
            rows,
            height,
            window,
        });
        window
    }

    /// Records, once layout has placed this frame's tree, whether a
    /// virtualized block's view shows a row its build left out; see
    /// [`Ui::needs_rebuild`].
    pub(in crate::tree) fn check_built_rows(&mut self) {
        self.rows_missed = self.built_rows.iter().any(|built| {
            let stored = self.store.get(self.nodes[built.node].slot);
            let (start, end) = stored.content_box();
            let offset = stored.scroll_offset.y.max(0.0);
            let shown = window_of(built.rows, built.height, offset, end.y - start.y, 0);
            shown.first < shown.end
                && (shown.first < built.window.first || shown.end > built.window.end)
        });
    }

    /// **Whether the last [`Ui::layout`] showed a row this frame's build did
    /// not build**, so that drawing now would leave a gap in a [`Ui::list`] or
    /// an [`Ui::outliner`]: build and lay out once more, and the next build
    /// windows the view layout just measured.
    ///
    /// A list windows its rows from this frame's row count and offset, so a
    /// change to either needs no second build. What a build cannot know is a
    /// view taller than the one last laid out: a list's very first layout
    /// when nothing was laid out before it, or a list whose box grew by more
    /// than its [`LIST_OVERSCAN`] rows — one sized by its content, or a
    /// sibling that gave up its space. False before the first layout.
    #[must_use]
    pub const fn needs_rebuild(&self) -> bool {
        self.rows_missed
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

/// The rows of `rows` rows `height` pixels tall that a view `view` pixels
/// tall shows at `offset`, and `overscan` more on each side.
fn window_of(rows: usize, height: f32, offset: f32, view: f32, overscan: usize) -> RowWindow {
    // Float-to-integer casts saturate, so a huge offset is the last row.
    let first = ((offset / height).floor() as usize).saturating_sub(overscan);
    let last = ((offset + view.max(0.0)) / height).ceil() as usize;
    let end = last.saturating_add(overscan).min(rows);
    RowWindow {
        first: first.min(end),
        end,
    }
}

/// A virtualized block this frame built, and the window it built: what
/// [`Ui::check_built_rows`] holds the laid-out view to.
#[derive(Clone, Copy, Debug)]
pub(in crate::tree) struct BuiltRows {
    /// The block's index in this frame's nodes.
    node: usize,
    /// Its row count this frame.
    rows: usize,
    /// Its row height, as [`row_span`](list::row_span) made it usable.
    height: f32,
    /// The rows it built, the kept focused row aside.
    window: RowWindow,
}
