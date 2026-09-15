//! A virtualized list of rows of one fixed height.
//!
//! The list is an `overflow: scroll` block holding one `.list-content` block
//! as tall as every row together, so the scroll offset clamps to the whole
//! list; only the rows that last frame's view showed at this frame's offset,
//! and [`LIST_OVERSCAN`] more on each side, are built, each absolutely
//! positioned at its index times the row height. The row height being fixed
//! is what makes that window arithmetic rather than a measurement: Unity's
//! `ListView` virtualizes only at a fixed height for the same reason.
//!
//! **Focus moves through rows that are not built.** The overscan rows are in
//! the tree, clipped out of sight, so a step past the last row shown lands on
//! one; the list is a scroll container, so focusing it scrolls it into view,
//! and the next frame's window follows the offset. The row that holds focus is
//! kept, and built even when the offset has moved away from it, so scrolling
//! never takes focus away.

use super::{Ui, WidgetState, typed};
use crate::style::{Declaration, Sides};
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
        mut row: impl FnMut(&mut Self, usize),
    ) -> Response {
        let selector = typed("list", selector);
        let height = if row_height.is_finite() {
            row_height.max(1.0)
        } else {
            1.0
        };
        let mut focused = None;
        let response = self.block_with(
            &selector,
            &[Declaration::Overflow(Overflow::Scroll)],
            Behavior::NONE,
            |ui| focused = ui.list_rows(rows, height, &mut row),
        );
        self.set_widget_state(response.key, WidgetState::List(focused));
        response
    }

    /// The inside of an open list block: its content and the rows in the
    /// window. Returns the index of the row that holds focus.
    fn list_rows(
        &mut self,
        rows: usize,
        height: f32,
        row: &mut impl FnMut(&mut Self, usize),
    ) -> Option<usize> {
        let list = &self.nodes[*self.open.last().expect("called inside the list block")];
        let stored = self.store.get(list.slot);
        let view = if list.fresh {
            // Nothing laid out yet: the stylesheet's height, when it is one.
            match list.style.height {
                LengthAuto::Px(px) => px,
                LengthAuto::Percent(_) | LengthAuto::Auto => 0.0,
            }
        } else {
            let (start, end) = stored.content_box();
            end.y - start.y
        };
        let offset = stored.scroll_offset.y.max(0.0);
        let kept = match stored.widget {
            WidgetState::List(focused) => focused.filter(|&index| index < rows),
            _ => None,
        };

        // Float-to-integer casts saturate, so a huge offset is the last row.
        let first = ((offset / height).floor() as usize).saturating_sub(LIST_OVERSCAN);
        let last = ((offset + view.max(0.0)) / height).ceil() as usize;
        let end = last.saturating_add(LIST_OVERSCAN).min(rows);
        let first = first.min(end);

        let content = [
            Declaration::Height(LengthAuto::Px(rows as f32 * height)),
            Declaration::FlexShrink(0.0),
        ];
        let mut focused = None;
        self.block(".list-content", &content, |ui| {
            // The focused row outside the window, built in its place in tree
            // order so that next and previous still walk the rows in order.
            let outside = kept.filter(|&index| {
                (index < first || index >= end)
                    && ui.focused() == Some(ui.key(KeySource::Keyed(hash_of(index))))
            });
            let indices = outside
                .filter(|&index| index < first)
                .into_iter()
                .chain(first..end)
                .chain(outside.filter(|&index| index >= end));
            for index in indices {
                let top = index as f32 * height;
                let inline = [
                    Declaration::Position(Position::Absolute),
                    Declaration::Inset(Sides::Top, LengthAuto::Px(top)),
                    Declaration::Inset(Sides::Left, LengthAuto::Px(0.0)),
                    Declaration::Inset(Sides::Right, LengthAuto::Px(0.0)),
                    Declaration::Height(LengthAuto::Px(height)),
                ];
                let response =
                    ui.block_keyed_with(index, ".list-row", &inline, Behavior::BUTTON, |ui| {
                        row(ui, index);
                    });
                if response.focused {
                    focused = Some(index);
                }
            }
        });
        focused
    }
}
