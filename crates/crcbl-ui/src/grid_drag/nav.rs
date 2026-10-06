//! The pad's and the keyboard's focus over grids: a step inside one grid, and
//! a step off its edge onto a linked one.
//!
//! Resolved against the shapes of the grids the previous frame ran, as the
//! tree resolves its focus against the tree the previous frame built, so a
//! step lands before any grid of this frame draws.

use glam::UVec2;

use super::{CellGrid, GridCell};
use crate::tree::Direction;
use crate::widget::WidgetId;

/// What a step needs of a grid: which it is, how many cells it has, and the
/// first cell it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GridShape {
    id_base: WidgetId,
    columns: u32,
    rows: u32,
    first: UVec2,
}

impl GridShape {
    /// `grid`'s shape.
    pub(super) fn of(grid: &CellGrid) -> Self {
        Self {
            id_base: grid.id_base,
            columns: grid.columns,
            rows: grid.rows,
            first: grid.shown().first,
        }
    }

    /// Whether `at` is one of this grid's cells.
    pub(super) fn holds(&self, at: GridCell) -> bool {
        at.grid == self.id_base && at.cell.x < self.columns && at.cell.y < self.rows
    }

    /// Where focus lands on this grid when nothing was focused: the first cell
    /// it shows. `None` for a grid with no cells.
    pub(super) fn landing(&self) -> Option<GridCell> {
        let at = GridCell {
            grid: self.id_base,
            cell: self.first,
        };
        self.holds(at).then_some(at)
    }
}

/// A step off `from`'s edge `toward` a side lands on `to`. See
/// [`GridDrag::link`](super::GridDrag::link).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GridLink {
    pub(super) from: WidgetId,
    pub(super) toward: Direction,
    pub(super) to: WidgetId,
}

/// Where one step `toward` a side takes focus from `at`.
///
/// The next cell of the same grid; off its edge, the near edge of the grid
/// linked that way, on the row or column nearest the one left; and where
/// neither is, `at` itself — focus stops at an edge rather than wrapping.
pub(super) fn step(
    layout: &[GridShape],
    links: &[GridLink],
    at: GridCell,
    toward: Direction,
) -> GridCell {
    let Some(shape) = layout.iter().find(|shape| shape.id_base == at.grid) else {
        return at;
    };
    let cell = at.cell;
    let inside = match toward {
        Direction::Up => cell.y.checked_sub(1).map(|y| UVec2::new(cell.x, y)),
        Direction::Down => cell
            .y
            .checked_add(1)
            .filter(|&y| y < shape.rows)
            .map(|y| UVec2::new(cell.x, y)),
        Direction::Left => cell.x.checked_sub(1).map(|x| UVec2::new(x, cell.y)),
        Direction::Right => cell
            .x
            .checked_add(1)
            .filter(|&x| x < shape.columns)
            .map(|x| UVec2::new(x, cell.y)),
    };
    if let Some(cell) = inside {
        return GridCell {
            grid: at.grid,
            cell,
        };
    }
    let next = links
        .iter()
        .find(|link| link.from == at.grid && link.toward == toward)
        .and_then(|link| layout.iter().find(|shape| shape.id_base == link.to))
        .filter(|next| next.columns > 0 && next.rows > 0);
    let Some(next) = next else {
        return at;
    };
    let column = cell.x.min(next.columns - 1);
    let row = cell.y.min(next.rows - 1);
    let cell = match toward {
        Direction::Up => UVec2::new(column, next.rows - 1),
        Direction::Down => UVec2::new(column, 0),
        Direction::Left => UVec2::new(next.columns - 1, row),
        Direction::Right => UVec2::new(0, row),
    };
    GridCell {
        grid: next.id_base,
        cell,
    }
}
