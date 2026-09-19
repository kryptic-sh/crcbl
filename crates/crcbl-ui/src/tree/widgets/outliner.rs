//! A virtualized outliner: a tree of rows over the same fixed-row-height
//! window [`Ui::list`] builds, so a hundred thousand rows cost what a hundred
//! do.
//!
//! # The shape this follows, and why
//!
//! An outliner is a **flattened visible-row model** — one `Vec` of the rows an
//! expansion state leaves showing — virtualized at a fixed row height, which is
//! how Unity's `TreeView` and Blender's outliner are built. It is not Godot's
//! `Tree`, which keeps a retained item per node and lays every one of them out,
//! and so stalls past ten thousand rows; and it is not a variable-height
//! virtualizer, because a fixed height turns "which rows are in view" into
//! arithmetic instead of a measurement — the reason Unity's `ListView`
//! virtualizes only at a fixed height. Nesting is drawn as `padding-left`, so
//! depth costs a number rather than a block per level.
//!
//! # Who owns what
//!
//! [`OutlinerState`] is the application's: it holds which items are expanded,
//! which are selected, and the flattened rows. The **caller** flattens, through
//! the [`OutlinerBuilder`] the widget hands it, and a collapsed branch's
//! children are never walked — so the flatten costs what is visible, not what
//! the tree holds. The flatten runs only when [`OutlinerState::is_stale`], which
//! expanding, collapsing and [`OutlinerState::invalidate`] set, so a still frame
//! walks nothing at all and builds only the rows in its window.
//!
//! # What the widget answers
//!
//! * **The pointer**: a click on a row's `.outliner-toggle` expands or
//!   collapses it; a click anywhere else on the row selects it. The toggle is a
//!   button that cannot take focus, so the press latches on it rather than on
//!   the row, and navigation stops once per row rather than twice.
//! * **The keyboard and the pad**: rows are [`Behavior::BUTTON`]s, so focus
//!   walks them and accept selects. Left and right expand and collapse through
//!   the same WAI-ARIA tree view rule [`Ui::tree_node`] follows —
//!   [`Ui::tree_item_step`](Ui) reads the [`WidgetState::TreeItem`] each row
//!   keeps — and an expansion it makes is answered at the top of the next
//!   [`Ui::outliner`] call, before the model is flattened, so the children show
//!   in the same frame.
//! * **The selection** is single by default. [`SelectMode`] is the frame's
//!   modifier, mapped by the caller from its own input exactly as
//!   [`NavInput`](crate::tree::NavInput) is: `Toggle` adds or removes one row,
//!   `Range` takes every row between the anchor and the one acted on.

use std::collections::HashSet;
use std::panic::Location;

use super::list::{row_inline, row_span};
use super::{Ui, WidgetState, typed};
use crate::style::{Declaration, PseudoClasses, Sides};
use crate::tree::{
    Behavior, KeySource, Length, LengthAuto, NodeKey, Overflow, Response, Role, hash_of,
};

/// How far one level of depth indents a row, in pixels:
/// [`OutlinerOptions::indent`]'s default.
pub const OUTLINER_INDENT: f32 = 12.0;

/// How tall a row is, in pixels: [`OutlinerOptions::row_height`]'s default.
pub const OUTLINER_ROW_HEIGHT: f32 = 16.0;

/// An item's identity in an outliner: what expansion and selection are kept
/// by, and what keys its row, so both follow the item when the rows reorder.
///
/// The application mints these — a scene entity id, a hash of a path — and only
/// has to keep them stable and distinct within one [`OutlinerState`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutlinerId(pub u64);

/// One row of an outliner's flattened visible model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutlinerRow {
    /// The item the row stands for.
    pub id: OutlinerId,
    /// The item this one is a child of; none at a root.
    pub parent: Option<OutlinerId>,
    /// How deep it sits, `0` at a root: what it is indented by.
    pub depth: u16,
    /// Whether it can be expanded at all.
    pub branch: bool,
    /// Whether it is expanded, so its children are in the model under it.
    pub open: bool,
}

/// How a click or an accept on a row changes the selection this frame.
///
/// The tree reads no device, so the caller maps its own modifier keys to these
/// as it maps a [`NavInput`](crate::tree::NavInput) — control for [`Self::Toggle`],
/// shift for [`Self::Range`], in the convention every file manager shares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SelectMode {
    /// The row becomes the only selected row, and the anchor.
    #[default]
    Replace,
    /// The row joins the selection, or leaves it, and becomes the anchor.
    Toggle,
    /// Every row between the anchor and this one is selected, and the anchor
    /// stays where it was. With no anchor this is [`Self::Replace`].
    Range,
}

/// What [`Ui::outliner_with`] takes beside its rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlinerOptions {
    /// How tall each row is, in pixels; under one pixel is taken as one.
    pub row_height: f32,
    /// How far one level of depth indents a row, in pixels.
    pub indent: f32,
    /// How a click or an accept this frame changes the selection.
    pub select: SelectMode,
}

impl Default for OutlinerOptions {
    fn default() -> Self {
        Self {
            row_height: OUTLINER_ROW_HEIGHT,
            indent: OUTLINER_INDENT,
            select: SelectMode::Replace,
        }
    }
}

/// An outliner's expansion, selection and flattened rows: the application's to
/// keep, and the thing a saved outliner is saved from. See the module docs.
#[derive(Clone, Debug)]
pub struct OutlinerState {
    expanded: HashSet<OutlinerId>,
    selected: HashSet<OutlinerId>,
    rows: Vec<OutlinerRow>,
    /// Where a [`SelectMode::Range`] runs from.
    anchor: Option<OutlinerId>,
    /// Whether the rows must be flattened again before they are read.
    stale: bool,
}

impl Default for OutlinerState {
    fn default() -> Self {
        Self::new()
    }
}

impl OutlinerState {
    /// Nothing expanded, nothing selected, and the rows still to be flattened.
    #[must_use]
    pub fn new() -> Self {
        Self {
            expanded: HashSet::new(),
            selected: HashSet::new(),
            rows: Vec::new(),
            anchor: None,
            stale: true,
        }
    }

    /// The flattened visible rows, in the order they are shown.
    #[must_use]
    pub fn rows(&self) -> &[OutlinerRow] {
        &self.rows
    }

    /// Whether the rows must be flattened again: set by every change to the
    /// expansion and by [`Self::invalidate`], cleared by [`Self::flatten`].
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    /// Says the tree the rows were flattened from has changed, so that the next
    /// [`Ui::outliner`] flattens it again.
    pub const fn invalidate(&mut self) {
        self.stale = true;
    }

    /// Flattens the rows again: `build` walks the caller's tree, pushing a row
    /// per visible item, and a branch's children are walked only while it is
    /// expanded. Clears [`Self::is_stale`].
    pub fn flatten(&mut self, build: impl FnOnce(&mut OutlinerBuilder<'_>)) {
        self.rows.clear();
        self.stale = false;
        let mut builder = OutlinerBuilder {
            rows: &mut self.rows,
            expanded: &self.expanded,
            depth: 0,
            parent: None,
        };
        build(&mut builder);
    }

    /// Whether `id`'s children are shown.
    #[must_use]
    pub fn is_expanded(&self, id: OutlinerId) -> bool {
        self.expanded.contains(&id)
    }

    /// Shows or hides `id`'s children. Returns whether that changed anything,
    /// and makes the rows stale when it did.
    pub fn set_expanded(&mut self, id: OutlinerId, expanded: bool) -> bool {
        let moved = if expanded {
            self.expanded.insert(id)
        } else {
            self.expanded.remove(&id)
        };
        self.stale |= moved;
        moved
    }

    /// [`Self::set_expanded`] to the opposite of what `id` is now; returns what
    /// it became.
    pub fn toggle_expanded(&mut self, id: OutlinerId) -> bool {
        let expanded = !self.is_expanded(id);
        self.set_expanded(id, expanded);
        expanded
    }

    /// Whether `id` is selected.
    #[must_use]
    pub fn is_selected(&self, id: OutlinerId) -> bool {
        self.selected.contains(&id)
    }

    /// How many items are selected.
    #[must_use]
    pub fn selected_len(&self) -> usize {
        self.selected.len()
    }

    /// The selected items, in the order the flattened rows show them; an item
    /// selected while a collapsed branch hides it is not among them.
    pub fn selected(&self) -> impl Iterator<Item = OutlinerId> + '_ {
        self.rows
            .iter()
            .map(|row| row.id)
            .filter(|id| self.selected.contains(id))
    }

    /// Where a [`SelectMode::Range`] runs from: the row last selected outright.
    #[must_use]
    pub const fn anchor(&self) -> Option<OutlinerId> {
        self.anchor
    }

    /// Selects nothing, and forgets the anchor.
    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    /// Acts on `id` as `mode` says; see [`SelectMode`]. Returns whether the
    /// selection changed.
    ///
    /// A [`SelectMode::Range`] walks the flattened rows to find the anchor, so
    /// it costs the model's length; the other two are constant.
    pub fn select(&mut self, id: OutlinerId, mode: SelectMode) -> bool {
        match mode {
            SelectMode::Replace => {
                let already = self.selected.len() == 1 && self.selected.contains(&id);
                self.selected.clear();
                self.selected.insert(id);
                self.anchor = Some(id);
                !already
            }
            SelectMode::Toggle => {
                if !self.selected.remove(&id) {
                    self.selected.insert(id);
                }
                self.anchor = Some(id);
                true
            }
            SelectMode::Range => {
                let Some(anchor) = self.anchor else {
                    return self.select(id, SelectMode::Replace);
                };
                let from = self.rows.iter().position(|row| row.id == anchor);
                let to = self.rows.iter().position(|row| row.id == id);
                let (Some(from), Some(to)) = (from, to) else {
                    return self.select(id, SelectMode::Replace);
                };
                let (from, to) = (from.min(to), from.max(to));
                let wanted: HashSet<OutlinerId> =
                    self.rows[from..=to].iter().map(|row| row.id).collect();
                let moved = wanted != self.selected;
                self.selected = wanted;
                moved
            }
        }
    }
}

/// Flattens an outliner's visible rows; see [`OutlinerState::flatten`].
#[derive(Debug)]
pub struct OutlinerBuilder<'a> {
    rows: &'a mut Vec<OutlinerRow>,
    expanded: &'a HashSet<OutlinerId>,
    depth: u16,
    parent: Option<OutlinerId>,
}

impl OutlinerBuilder<'_> {
    /// Pushes an item with children, and runs `children` one level deeper —
    /// **only while the item is expanded**, so a collapsed subtree is never
    /// walked. Returns whether it is expanded.
    pub fn branch(&mut self, id: OutlinerId, children: impl FnOnce(&mut Self)) -> bool {
        let open = self.expanded.contains(&id);
        self.rows.push(OutlinerRow {
            id,
            parent: self.parent,
            depth: self.depth,
            branch: true,
            open,
        });
        if open {
            let (depth, parent) = (self.depth, self.parent);
            self.depth = depth.saturating_add(1);
            self.parent = Some(id);
            children(self);
            self.depth = depth;
            self.parent = parent;
        }
        open
    }

    /// Pushes an item with no children, which neither a click on its toggle nor
    /// right opens.
    pub fn leaf(&mut self, id: OutlinerId) {
        self.rows.push(OutlinerRow {
            id,
            parent: self.parent,
            depth: self.depth,
            branch: false,
            open: false,
        });
    }

    /// How deep the rows pushed now sit.
    #[must_use]
    pub const fn depth(&self) -> u16 {
        self.depth
    }
}

impl Ui {
    /// A virtualized outliner over `state`'s rows, each [`OUTLINER_ROW_HEIGHT`]
    /// tall and indented [`OUTLINER_INDENT`] a level, selecting one row at a
    /// time. [`Ui::outliner_with`] for the rest.
    #[track_caller]
    pub fn outliner(
        &mut self,
        selector: &str,
        state: &mut OutlinerState,
        tree: impl FnOnce(&mut OutlinerBuilder<'_>),
        row: impl FnMut(&mut Self, &OutlinerRow),
    ) -> Response {
        self.outliner_with(selector, state, &OutlinerOptions::default(), tree, row)
    }

    /// A virtualized outliner: an `outliner` block — `overflow: scroll`, sized
    /// by its stylesheet — holding an `.outliner-content` block as tall as
    /// every row, holding a focusable `.outliner-row` block per row in the
    /// window, each an `.outliner-toggle` block showing `+`, `-` or a space and
    /// then whatever `row` builds.
    ///
    /// `tree` flattens the caller's tree into `state`, and is run only when
    /// [`OutlinerState::is_stale`] — after this frame's expand and collapse are
    /// applied, so a row opened this frame shows its children this frame. A row
    /// is `:open` while expanded and `:checked` while selected. Returns the
    /// block's [`Response`], with [`Response::changed`] set the frame the
    /// expansion or the selection moved.
    #[track_caller]
    pub fn outliner_with(
        &mut self,
        selector: &str,
        state: &mut OutlinerState,
        options: &OutlinerOptions,
        tree: impl FnOnce(&mut OutlinerBuilder<'_>),
        mut row: impl FnMut(&mut Self, &OutlinerRow),
    ) -> Response {
        let selector = typed("outliner", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());

        let mut changed = self.apply_expansion(key, state);
        if state.is_stale() {
            state.flatten(tree);
        }

        let height = row_span(options.row_height);
        let mut focused = None;
        let mut response = self.open_block(
            key,
            parsed,
            &[Declaration::Overflow(Overflow::Scroll)],
            Behavior::NONE,
            PseudoClasses::NONE,
            |ui| {
                let (built, moved) = ui.outliner_rows(key, state, options, height, &mut row);
                focused = built;
                changed |= moved;
            },
        );
        self.set_widget_state(key, WidgetState::Rows(focused));
        response.changed = changed;
        response
    }

    /// Answers the expand and collapse this frame's input asked of the outliner
    /// on `key` — a click on a row's toggle, and the left or right a focused
    /// row took through the tree view rule — before the rows are flattened.
    /// Returns whether the expansion moved.
    fn apply_expansion(&mut self, key: NodeKey, state: &mut OutlinerState) -> bool {
        if self.building_disabled() {
            return false;
        }
        let mut changed = false;
        // Only the row the tree view rule actually toggled this frame, never
        // whatever the focused row's last build left in the store: the caller
        // may have collapsed it from outside since, and reading the store back
        // unconditionally would undo that.
        if let Some(toggled) = self.tree_toggled
            && self.inside(toggled, Some(key))
            && let WidgetState::TreeItem {
                open,
                item: Some(id),
                ..
            } = self.widget_state(toggled)
        {
            changed |= state.set_expanded(id, open);
        }
        if let Some(clicked) = self.clicked_key()
            && self.inside(clicked, Some(key))
            && let WidgetState::OutlinerToggle(id) = self.widget_state(clicked)
        {
            state.toggle_expanded(id);
            changed = true;
        }
        changed
    }

    /// The inside of an open outliner block: its content and the rows in the
    /// window. Returns the index of the row that holds focus, and whether the
    /// selection moved.
    fn outliner_rows(
        &mut self,
        key: NodeKey,
        state: &mut OutlinerState,
        options: &OutlinerOptions,
        height: f32,
        row: &mut impl FnMut(&mut Self, &OutlinerRow),
    ) -> (Option<usize>, bool) {
        let rows = state.rows().len();
        let window = self.row_window(rows, height);
        let content = [
            Declaration::Height(LengthAuto::Px(rows as f32 * height)),
            Declaration::FlexShrink(0.0),
        ];
        let disabled = self.building_disabled();
        let (mut focused, mut changed) = (None, false);
        self.block(".outliner-content", &content, |ui| {
            // The focused row outside the window, built in its place in tree
            // order so that next and previous still walk the rows in order.
            // The model is read by index, never walked: a hundred thousand
            // rows must cost this loop what a hundred do.
            let model: &[OutlinerRow] = state.rows();
            let outside = ui
                .kept_row(key, rows, |ui, index| {
                    ui.key(KeySource::Keyed(hash_of(model[index].id)))
                })
                .filter(|&index| index < window.first || index >= window.end);
            let indices = outside
                .filter(|&index| index < window.first)
                .into_iter()
                .chain(window.first..window.end)
                .chain(outside.filter(|&index| index >= window.end));
            for index in indices {
                let item = state.rows()[index];
                let (row_key, clicked) = ui.outliner_row(index, item, state, options, height, row);
                if clicked && !disabled {
                    changed |= state.select(item.id, options.select);
                }
                if ui.focused() == Some(row_key) {
                    focused = Some(index);
                }
            }
        });
        (focused, changed)
    }

    /// One row of an outliner, inside its content block: the row block, its
    /// toggle and whatever `row` builds. Returns the row's key and whether it
    /// was clicked.
    fn outliner_row(
        &mut self,
        index: usize,
        item: OutlinerRow,
        state: &OutlinerState,
        options: &OutlinerOptions,
        height: f32,
        row: &mut impl FnMut(&mut Self, &OutlinerRow),
    ) -> (NodeKey, bool) {
        let step = if options.indent.is_finite() {
            options.indent.max(0.0)
        } else {
            0.0
        };
        let [position, top, left, right, height] = row_inline(index, height);
        let inline = [
            position,
            top,
            left,
            right,
            height,
            Declaration::Padding(Sides::Left, Length::Px(f32::from(item.depth) * step)),
        ];

        let open = if item.open {
            PseudoClasses::OPEN
        } else {
            PseudoClasses::NONE
        };
        let selected = if state.is_selected(item.id) {
            PseudoClasses::CHECKED
        } else {
            PseudoClasses::NONE
        };

        // The tree view rule reads these off the focused row: its parent is
        // whichever row the flatten recorded, and its first child is the next
        // row, when that row is its child. Every row of one outliner is keyed
        // by its item under the same content block, so both keys are known
        // before either row is built.
        let keyed = |ui: &Self, id: OutlinerId| ui.key(KeySource::Keyed(hash_of(id)));
        let parent = item.parent.map(|id| keyed(self, id));
        let first_child = state
            .rows()
            .get(index + 1)
            .filter(|next| next.parent == Some(item.id))
            .map(|next| keyed(self, next.id));

        let parsed = self.node_selector(".outliner-row");
        let key = keyed(self, item.id);
        let key = self.unique(key);
        let response = self.open_block(
            key,
            parsed,
            &inline,
            Behavior::BUTTON,
            open | selected,
            |ui| {
                ui.outliner_toggle(item);
                row(ui, &item);
            },
        );
        self.set_widget_state(
            key,
            WidgetState::TreeItem {
                open: item.open,
                branch: item.branch,
                parent,
                first_child,
                item: Some(item.id),
            },
        );
        (key, response.clicked)
    }

    /// A row's toggle: a `.outliner-toggle` block holding a
    /// `.outliner-toggle-glyph` span of `+`, `-` or a space. A branch's toggle
    /// is a button that cannot take focus, so a click on it expands the row
    /// rather than selecting it, and a leaf's takes no press at all.
    fn outliner_toggle(&mut self, item: OutlinerRow) {
        let glyph = match (item.branch, item.open) {
            (false, _) => " ",
            (true, true) => "-",
            (true, false) => "+",
        };
        let behavior = if item.branch {
            Behavior {
                role: Role::Button,
                focusable: Some(false),
                ..Behavior::NONE
            }
        } else {
            Behavior::NONE
        };
        let toggle = self.block_with(".outliner-toggle", &[], behavior, |ui| {
            ui.span(".outliner-toggle-glyph", glyph, &[]);
        });
        if item.branch {
            self.set_widget_state(toggle.key, WidgetState::OutlinerToggle(item.id));
        }
    }
}
