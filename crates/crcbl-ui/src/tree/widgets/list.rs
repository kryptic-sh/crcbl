//! A virtualized list of rows of one fixed height.
//!
//! The list is an `overflow: scroll` block holding one `.list-content` block
//! as tall as every row together, so the scroll offset clamps to the whole
//! list; only the rows its view shows at this frame's offset, clamped to this
//! frame's row count, and [`LIST_OVERSCAN`] more on each side, are built, each
//! absolutely positioned at its index times the row height. The row height
//! being fixed is what makes that window arithmetic rather than a measurement:
//! Unity's `ListView` virtualizes only at a fixed height for the same reason.
//! [`Ui::row_window`](super::Ui::row_window) is the arithmetic, shared with
//! [`Ui::outliner`].
//!
//! **One build is enough after the rows or the offset change**, since both are
//! this frame's. The view's height is the one thing a build cannot know before
//! layout: it is the height last laid out, or for a list never laid out its
//! stylesheet's length or the viewport. When layout then shows a row the build
//! left out — a first frame, or a list that grew taller — the
//! [`Ui::needs_rebuild`] it reports asks for one more build.
//!
//! **Focus moves through rows that are not built.** The overscan rows are in
//! the tree, clipped out of sight, so a step past the last row shown lands on
//! one; the list is a scroll container, so focusing it scrolls it into view,
//! and the next frame's window follows the offset. The row that holds focus is
//! kept, and built even when the offset has moved away from it, so scrolling
//! never takes focus away.

use super::{Ui, WidgetKind, WidgetState, typed};
use crate::style::{Declaration, PseudoClasses, Sides};
use crate::tree::{Behavior, KeySource, LengthAuto, Overflow, Position, Response, hash_of};

/// How many rows a list builds past each edge of its view: enough that a step
/// past the view's last row always finds a row to land on.
pub const LIST_OVERSCAN: usize = 2;

impl Ui {
    /// A virtualized list of `rows` rows, each `row_height` pixels tall: a
    /// `list` block — `overflow: scroll`, sized by its stylesheet — holding a
    /// `.list-content` block, holding a focusable `.list-row` block for each
    /// row it builds, which `row` fills with that row's index. Inside `row`,
    /// [`Ui::clicked`] and its kin read the row. See the module docs for which
    /// rows are built; a `row_height` under one pixel is taken as one.
    #[track_caller]
    pub fn list(
        &mut self,
        selector: &str,
        rows: usize,
        row_height: f32,
        row: impl FnMut(&mut Self, usize),
    ) -> Response {
        self.selectable_list(selector, rows, row_height, None, row)
    }

    /// [`Ui::list`] with one row selected: row `selected` is built
    /// `:checked`, for the stylesheet to draw as `.list-row:checked`, and a
    /// screen reader hears it as selected. For a list whose current row is the
    /// application's — a launcher's result Return accepts — rather than
    /// wherever focus is.
    #[track_caller]
    pub fn selectable_list(
        &mut self,
        selector: &str,
        rows: usize,
        row_height: f32,
        selected: Option<usize>,
        mut row: impl FnMut(&mut Self, usize),
    ) -> Response {
        let selector = typed("list", selector);
        let height = row_span(row_height);
        let mut focused = None;
        let response = self.block_with(
            &selector,
            &[Declaration::Overflow(Overflow::Scroll)],
            Behavior::NONE,
            |ui| focused = ui.list_rows(rows, height, selected, &mut row),
        );
        self.set_widget_state(response.key, WidgetState::Rows(focused));
        self.mark(response.key, WidgetKind::List);
        response
    }

    /// The inside of an open list block: its content and the rows in the
    /// window. Returns the index of the row that holds focus.
    fn list_rows(
        &mut self,
        rows: usize,
        height: f32,
        selected: Option<usize>,
        row: &mut impl FnMut(&mut Self, usize),
    ) -> Option<usize> {
        let list = self.nodes[*self.open.last().expect("called inside the list block")].key;
        let window = self.row_window(rows, height);

        let content = [
            Declaration::Height(LengthAuto::Px(rows as f32 * height)),
            Declaration::FlexShrink(0.0),
        ];
        let mut focused = None;
        self.block(".list-content", &content, |ui| {
            // The focused row outside the window, built in its place in tree
            // order so that next and previous still walk the rows in order.
            let outside = ui
                .kept_row(list, rows, |ui, index| {
                    ui.key(KeySource::Keyed(hash_of(index)))
                })
                .filter(|&index| index < window.first || index >= window.end);
            let indices = outside
                .filter(|&index| index < window.first)
                .into_iter()
                .chain(window.first..window.end)
                .chain(outside.filter(|&index| index >= window.end));
            for index in indices {
                let inline = row_inline(index, height);
                let state = if selected == Some(index) {
                    PseudoClasses::CHECKED
                } else {
                    PseudoClasses::NONE
                };
                let parsed = ui.node_selector(".list-row");
                let key = ui.key(KeySource::Keyed(hash_of(index)));
                let response = ui.open_block(key, parsed, &inline, Behavior::BUTTON, state, |ui| {
                    row(ui, index);
                });
                ui.mark(response.key, WidgetKind::ListRow);
                if response.focused {
                    focused = Some(index);
                }
            }
        });
        focused
    }
}

/// `row_height` as a usable row span: a height under one pixel, or one that is
/// not finite, is taken as one pixel.
pub(super) fn row_span(row_height: f32) -> f32 {
    if row_height.is_finite() {
        row_height.max(1.0)
    } else {
        1.0
    }
}

/// The inline declarations that place row `index` of a virtualized block: a
/// full-width absolute box at its index times `height`.
pub(super) fn row_inline(index: usize, height: f32) -> [Declaration; 5] {
    [
        Declaration::Position(Position::Absolute),
        Declaration::Inset(Sides::Top, LengthAuto::Px(index as f32 * height)),
        Declaration::Inset(Sides::Left, LengthAuto::Px(0.0)),
        Declaration::Inset(Sides::Right, LengthAuto::Px(0.0)),
        Declaration::Height(LengthAuto::Px(height)),
    ]
}
