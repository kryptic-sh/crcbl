//! Typed drag-and-drop over grids of cells: [`CellGrid`], [`GridDrag`].
//!
//! ```text
//!   press over a cell ──▶ source(cell) ──▶ Grip { payload, origin }
//!                                              │  grab = cell − origin
//!                                              ▼
//!   each frame, the cell under the pointer ──▶ origin = hovered − grab
//!                                              │  can_accept(&payload, &target)
//!                                              ▼
//!                           DropFeedback::{Accepting, Refusing}   (widget state)
//!                                              │
//!   release over an accepting cell ───────────▶ Dropped { payload, from, to }
//! ```
//!
//! # Built on the press capture, not beside it
//!
//! [`UiState::interact`] already latches the widget a press started over, and
//! that capture is the whole of what makes a drag a drag: the cell the pointer
//! lets go on is a *different* question from the cell that owns the press.
//! [`CellGrid`] gives every cell a [`WidgetId`] and drives it through that one
//! capture, so a grid's cells, a menu's rows and a button share one answer to
//! "who owns this press", and a drag cannot start while a slider is held.
//!
//! What this module adds on top is what two panels had each written for
//! themselves: the per-cell hit test, the payload taken when the press latches
//! (the capture is cleared on the frame the button comes up, so the source is
//! only knowable before then), the release that ended where it began filtered
//! out, and the grab offset — where inside a footprint the pointer took hold,
//! so a `2×2` grabbed by its bottom-right cell lands with *that* cell under the
//! pointer rather than its origin.
//!
//! # Feedback is widget state, not a stylesheet
//!
//! A drop target's answer comes back as a [`DropFeedback`] on the hovered cell
//! of the [`GridResponse`], the same way a button's hover comes back as a
//! [`ButtonState`]: the panel decides what an accepting or refusing cell looks
//! like. That is egui's and imgui's shape, and it is `docs/backlog.md`'s
//! decision of 2026-09-06 — drag-drop is not waiting on a CSS subset.
//!
//! # One drag, any number of grids
//!
//! A [`GridDrag`] is retained by the caller across frames, like a
//! [`UiState`]. Each frame opens a [`DragFrame`] over both, runs
//! [`DragFrame::grid`] once per grid on screen, and [`DragFrame::finish`]es it
//! for the drop. The payload is held by the drag rather than by a grid, so a
//! press on one grid and a release on another — a stash and a backpack — is one
//! drag, reported with both ends' [`GridCell`]s. Grids are told apart by their
//! [`CellGrid::id_base`], which has to be distinct anyway for their cells not to
//! share a press capture.

use glam::{UVec2, Vec2};

use crate::widget::{ButtonState, PointerInput, UiState, WidgetId};

/// Where a grid of equal cells is on screen, and the widget ids its cells
/// answer to.
///
/// A cell is a rectangle, [`cell`](Self::cell) wide and high: square for an
/// inventory's grid, and a one-cell grid of any shape for a drop slot — a
/// weapon card, an armour or quickslot — so every target is dragged onto the
/// same way.
///
/// Cells are numbered from the top-left, `x` across and `y` down; the widget id
/// of a cell is [`id_base`](Self::id_base) plus its row-major index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellGrid {
    /// The top-left corner of cell `(0, 0)`, in screen pixels.
    pub origin: Vec2,
    /// One cell's width (`x`) and height (`y`), in screen pixels; use
    /// `Vec2::splat(side)` for square cells.
    pub cell: Vec2,
    /// How many cells across.
    pub columns: u32,
    /// How many cells down.
    pub rows: u32,
    /// The widget id of cell `(0, 0)`. Every id from here to
    /// `id_base + columns * rows - 1` is this grid's, so it must not overlap
    /// another grid's range or any other widget's id.
    pub id_base: WidgetId,
}

impl CellGrid {
    /// Where `cell` is drawn: its top-left and bottom-right corners.
    #[must_use]
    pub fn cell_bounds(&self, cell: UVec2) -> (Vec2, Vec2) {
        let at = self.origin + cell.as_vec2() * self.cell;
        (at, at + self.cell)
    }

    /// Which cell `pos` is over, or `None` for a point outside the grid.
    ///
    /// The hit test every drag on this grid uses, and the one a test should
    /// aim with, so a check that presses a cell is pressing the cell the frame
    /// resolved. A non-finite position is over nothing, rather than truncating
    /// to cell `(0, 0)`.
    #[must_use]
    pub fn cell_at(&self, pos: Vec2) -> Option<UVec2> {
        if !pos.is_finite() || !self.cell.is_finite() || self.cell.cmple(Vec2::ZERO).any() {
            return None;
        }
        let local = (pos - self.origin) / self.cell;
        if local.x < 0.0 || local.y < 0.0 {
            return None;
        }
        let cell = local.floor().as_uvec2();
        (cell.x < self.columns && cell.y < self.rows).then_some(cell)
    }

    /// The widget id of `cell`.
    #[must_use]
    pub fn id_of(&self, cell: UVec2) -> WidgetId {
        self.id_base
            + WidgetId::from(cell.y) * WidgetId::from(self.columns)
            + WidgetId::from(cell.x)
    }

    /// [`id_of`](Self::id_of) undone: the cell a widget id names, or `None` for
    /// an id that is not one of this grid's.
    #[must_use]
    pub fn cell_of(&self, id: WidgetId) -> Option<UVec2> {
        let index = id.checked_sub(self.id_base)?;
        let columns = WidgetId::from(self.columns);
        if columns == 0 {
            return None;
        }
        let x = u32::try_from(index % columns).ok()?;
        let y = u32::try_from(index / columns).ok()?;
        (y < self.rows).then_some(UVec2::new(x, y))
    }

    /// Every cell, row by row from the top-left.
    pub fn cells(&self) -> impl Iterator<Item = UVec2> {
        let columns = self.columns;
        (0..self.rows).flat_map(move |y| (0..columns).map(move |x| UVec2::new(x, y)))
    }
}

/// A cell on a particular grid: which grid, by its [`CellGrid::id_base`], and
/// which cell of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCell {
    /// The grid's [`CellGrid::id_base`].
    pub grid: WidgetId,
    /// The cell on it.
    pub cell: UVec2,
}

/// What a drag source hands over when a press latches on one of its cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grip<P> {
    /// What is being dragged, typed by the game.
    pub payload: P,
    /// The cell the payload is anchored at — a placement's top-left — which
    /// the pressed cell's offset from is the grab offset. The pressed cell
    /// itself for a payload with no footprint.
    pub origin: UVec2,
}

/// The drag in progress: its payload, where it started and where inside the
/// payload's footprint it was taken hold of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held<P> {
    payload: P,
    from: GridCell,
    widget: WidgetId,
    grab: UVec2,
    /// Whether the game changed the payload while it was held
    /// ([`Held::payload_mut`], [`Held::turn_quarter`]), which makes a release
    /// on the cell it started on a drop rather than a click.
    changed: bool,
}

impl<P> Held<P> {
    /// What is being dragged.
    #[must_use]
    pub fn payload(&self) -> &P {
        &self.payload
    }

    /// What is being dragged, for a game that changes it mid-drag — an item
    /// turned by a rotate key while it is held.
    ///
    /// **Borrowing it marks the drag changed**: from then on a release on the
    /// cell the drag started on is offered to `can_accept` as a drop, where an
    /// unchanged payload let go there is a click that drops nothing. Read with
    /// [`Held::payload`] to leave it unmarked.
    pub fn payload_mut(&mut self) -> &mut P {
        self.changed = true;
        &mut self.payload
    }

    /// Whether the payload was changed while it was held — see
    /// [`Held::payload_mut`].
    #[must_use]
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// The cell the press latched on.
    #[must_use]
    pub fn from(&self) -> GridCell {
        self.from
    }

    /// The pressed cell's offset from the payload's [`Grip::origin`]: where
    /// inside the footprint the pointer holds it.
    #[must_use]
    pub fn grab(&self) -> UVec2 {
        self.grab
    }

    /// Keeps the grab offset inside a footprint of `size` cells, for a payload
    /// whose footprint changed while it was held — turned a quarter, say, so a
    /// `3×1` held by its third cell becomes a `1×3` held by its first column.
    ///
    /// Each axis is clamped to the last cell on it; a zero-sized footprint
    /// grabs at its origin.
    pub fn refit(&mut self, size: UVec2) {
        self.grab = self.grab.min(size.saturating_sub(UVec2::ONE));
    }

    /// Turns the grab a quarter with its footprint, for a payload turned a
    /// quarter while it is held, so the same cell of it stays under the
    /// pointer. `size_before` is the footprint before the turn, `w × h`; it
    /// becomes `h × w`, and the grabbed cell `(column, row)` becomes
    /// `(h − 1 − row, column)` — the turn clockwise on a grid whose rows run
    /// down, as a rotate key turns an item.
    ///
    /// A `1×2` held by its top cell, `(0, 0)`, is held by `(1, 0)` once it lies
    /// `2×1`. A grab outside `size_before` is first kept inside it, as
    /// [`Held::refit`] keeps it. The drag is marked changed, as by
    /// [`Held::payload_mut`].
    pub fn turn_quarter(&mut self, size_before: UVec2) {
        self.refit(size_before);
        let (column, row) = (self.grab.x, self.grab.y);
        self.grab = UVec2::new(size_before.y.saturating_sub(1) - row, column);
        self.changed = true;
    }
}

/// What a drop target is asked about: the cell under the pointer, and where
/// the payload's [`Grip::origin`] would land if it were let go there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropTarget {
    /// The cell under the pointer.
    pub at: GridCell,
    /// The hovered cell minus the grab offset: where the payload's origin
    /// goes, on the same grid as [`at`](Self::at).
    pub origin: UVec2,
}

/// A drop that happened: the payload, the cell it was taken from and the
/// target it was let go on, which its grid accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dropped<P> {
    /// What was dragged.
    pub payload: P,
    /// The cell the press latched on.
    pub from: GridCell,
    /// Where it was let go.
    pub to: DropTarget,
}

/// How a held drag ended, from [`DragFrame::release`]: dropped or not, and
/// where it was let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Released<P> {
    /// What was dragged.
    pub payload: P,
    /// The cell the press latched on.
    pub from: GridCell,
    /// The cell it was let go over; `None` over no grid, or when the drag was
    /// cancelled by its capture being cleared.
    pub over: Option<GridCell>,
    /// Where the payload's origin would have landed on [`over`](Self::over),
    /// by the grab offset, as `can_accept` was asked: `None` when that puts
    /// the origin off the grid's top or left edge, and for a click — a release
    /// on the cell the drag started on with the payload unchanged.
    pub target: Option<DropTarget>,
    /// Whether `target`'s grid accepted it: a drop happened exactly when this
    /// is `true`, and [`DragFrame::finish`] would have returned it.
    pub accepted: bool,
}

/// What a hovered cell would do with the payload being dragged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DropFeedback {
    /// Nothing is being dragged over this cell, or it is the cell the drag
    /// started on — a release there is a click, not a drop.
    #[default]
    None,
    /// Letting go here drops the payload.
    Accepting,
    /// Letting go here does nothing: the target refused the payload, or the
    /// grab offset would put its origin off the top or left edge.
    Refusing,
}

/// The drag, retained across frames by its caller alongside the [`UiState`]
/// whose capture it rides on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridDrag<P> {
    held: Option<Held<P>>,
}

impl<P> Default for GridDrag<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> GridDrag<P> {
    /// A drag holding nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self { held: None }
    }

    /// The drag in progress, if one is.
    #[must_use]
    pub fn held(&self) -> Option<&Held<P>> {
        self.held.as_ref()
    }

    /// The drag in progress, for a game that turns its payload mid-drag.
    pub fn held_mut(&mut self) -> Option<&mut Held<P>> {
        self.held.as_mut()
    }

    /// Opens this frame's drag over `ui` and `pointer`.
    ///
    /// Call it before any widget sharing `ui` interacts this frame: the press
    /// that starts a drag is recognised by the capture being empty *before*
    /// the grids ran and one of their cells' *after*. A drag whose capture was
    /// dropped since the last frame — [`UiState::clear`] on a torn-down panel —
    /// is dropped with it.
    pub fn frame<'a>(&'a mut self, ui: &'a mut UiState, pointer: PointerInput) -> DragFrame<'a, P> {
        let captured = ui.active();
        if self
            .held
            .as_ref()
            .is_some_and(|held| captured != Some(held.widget))
        {
            self.held = None;
        }
        DragFrame {
            drag: self,
            ui,
            pointer,
            captured,
            released: None,
        }
    }
}

/// One frame of a [`GridDrag`]: [`grid`](Self::grid) once per grid on screen,
/// then [`finish`](Self::finish).
#[derive(Debug)]
pub struct DragFrame<'a, P> {
    drag: &'a mut GridDrag<P>,
    ui: &'a mut UiState,
    pointer: PointerInput,
    /// The capture as it was before any grid ran this frame.
    captured: Option<WidgetId>,
    /// Where the pointer was released over a grid this frame, if it was.
    released: Option<ReleaseSpot>,
}

/// What a grid answered about the cell a held drag was released over.
#[derive(Debug, Clone, Copy)]
struct ReleaseSpot {
    over: GridCell,
    target: Option<DropTarget>,
    accepted: bool,
}

impl<P> DragFrame<'_, P> {
    /// Runs one grid's cells against this frame's pointer.
    ///
    /// `source` is asked, on the frame a press latches on one of this grid's
    /// cells, what that cell holds; `None` starts no drag, and nor does a
    /// [`Grip::origin`] below or right of the pressed cell, which no footprint
    /// covering that cell can have. `can_accept` is asked, while a drag is
    /// held, about the one cell under the pointer — never about the cell the
    /// drag started on unless the game changed the payload while it was held
    /// ([`Held::payload_mut`], [`Held::turn_quarter`]), and never when the grab
    /// offset puts the origin off the grid's top or left edge. Each is called
    /// at most once a frame.
    pub fn grid(
        &mut self,
        grid: &CellGrid,
        source: impl FnOnce(UVec2) -> Option<Grip<P>>,
        can_accept: impl FnOnce(&P, &DropTarget) -> bool,
    ) -> GridResponse {
        let pointer = self.pointer;
        let over = grid.cell_at(pointer.pos);
        let mut response = GridResponse {
            hovered: over,
            ..GridResponse::default()
        };

        // Only two cells of a grid can answer anything but `Idle`: the one
        // under the pointer, and the one holding the press. A cell that is
        // neither is a no-op through `interact`, so neither is asked.
        let pressed = self.ui.active().and_then(|id| grid.cell_of(id));
        for cell in over
            .into_iter()
            .chain(pressed.filter(|cell| over != Some(*cell)))
        {
            let hovered = over == Some(cell);
            let (state, _clicked) =
                self.ui
                    .interact(grid.id_of(cell), hovered, pointer.down, pointer.released);
            match state {
                ButtonState::Pressed => response.pressed = Some(cell),
                ButtonState::Hovered => response.lit = Some(cell),
                ButtonState::Idle => {}
            }
        }

        // The press latched this frame, on this grid: take hold.
        if self.drag.held.is_none()
            && self.captured.is_none()
            && pointer.down
            && let Some(id) = self.ui.active()
            && let Some(cell) = grid.cell_of(id)
            && let Some(grip) = source(cell)
            && let Some(grab) = cell.checked_sub(grip.origin)
        {
            self.drag.held = Some(Held {
                payload: grip.payload,
                from: GridCell {
                    grid: grid.id_base,
                    cell,
                },
                widget: id,
                grab,
                changed: false,
            });
        }

        let (Some(held), Some(cell)) = (self.drag.held.as_ref(), over) else {
            return response;
        };
        let at = GridCell {
            grid: grid.id_base,
            cell,
        };
        // Let go where it started, unchanged, it is a click; changed — turned
        // by a rotate key, say — it is a drop like any other.
        if at == held.from && !held.changed {
            if pointer.released {
                self.note_release(ReleaseSpot {
                    over: at,
                    target: None,
                    accepted: false,
                });
            }
            return response;
        }
        let target = cell
            .checked_sub(held.grab)
            .map(|origin| DropTarget { at, origin });
        let accepted = target.is_some_and(|target| can_accept(&held.payload, &target));
        if pointer.released {
            self.note_release(ReleaseSpot {
                over: at,
                target,
                accepted,
            });
        }
        response.drop = if accepted {
            DropFeedback::Accepting
        } else {
            DropFeedback::Refusing
        };
        response
    }

    /// Records what a grid answered about the cell released over. Grids may
    /// overlap — a one-cell slot drawn on top of a larger card — and each is
    /// run in turn, so **an accepting answer is kept against a later refusal or
    /// click**, and among answers of the same kind the later one wins: the drop
    /// happens if any grid under the pointer took it.
    fn note_release(&mut self, spot: ReleaseSpot) {
        if spot.accepted || !self.released.is_some_and(|kept| kept.accepted) {
            self.released = Some(spot);
        }
    }

    /// Ends the frame: the drop, if the pointer was released over a cell that
    /// accepted the payload.
    ///
    /// A release anywhere ends the drag, over an accepting cell or not — a
    /// release over nothing is a cancel. [`DragFrame::release`] reports every
    /// ending, refused ones included.
    pub fn finish(self) -> Option<Dropped<P>> {
        let released = self.release()?;
        let to = released.target.filter(|_| released.accepted)?;
        Some(Dropped {
            payload: released.payload,
            from: released.from,
            to,
        })
    }

    /// Ends the frame: how a held drag ended, if it ended this frame — dropped,
    /// refused, let go over nothing, or cancelled — for a game that tells the
    /// player why a drop did not happen. `None` while the drag goes on, and
    /// when nothing was held.
    pub fn release(self) -> Option<Released<P>> {
        let ended = self.pointer.released
            || self
                .drag
                .held
                .as_ref()
                .is_some_and(|held| self.ui.active() != Some(held.widget));
        if !ended {
            return None;
        }
        let held = self.drag.held.take()?;
        let spot = self.released;
        Some(Released {
            payload: held.payload,
            from: held.from,
            over: spot.map(|spot| spot.over),
            target: spot.and_then(|spot| spot.target),
            accepted: spot.is_some_and(|spot| spot.accepted),
        })
    }
}

/// What one grid answered this frame, for the panel drawing it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridResponse {
    /// The cell under the pointer, if it is over this grid.
    pub hovered: Option<UVec2>,
    /// The cell holding the press, if it is on this grid.
    pressed: Option<UVec2>,
    /// The cell under the pointer while nothing holds the press.
    lit: Option<UVec2>,
    /// What the hovered cell would do with the payload being dragged.
    drop: DropFeedback,
}

impl GridResponse {
    /// How `cell` should be drawn.
    #[must_use]
    pub fn cell(&self, cell: UVec2) -> CellResponse {
        let state = if self.pressed == Some(cell) {
            ButtonState::Pressed
        } else if self.lit == Some(cell) {
            ButtonState::Hovered
        } else {
            ButtonState::Idle
        };
        let drop = if self.hovered == Some(cell) {
            self.drop
        } else {
            DropFeedback::None
        };
        CellResponse { state, drop }
    }
}

/// How one cell should be drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellResponse {
    /// The cell as a button: [`Pressed`](ButtonState::Pressed) while it holds
    /// the press — for the whole drag, wherever the pointer is — and
    /// [`Hovered`](ButtonState::Hovered) under a pointer holding nothing.
    pub state: ButtonState,
    /// What it would do with the payload being dragged over it.
    pub drop: DropFeedback,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4×3 grid of 10-pixel cells at `(100, 50)`.
    const GRID: CellGrid = CellGrid {
        origin: Vec2::new(100.0, 50.0),
        cell: Vec2::splat(10.0),
        columns: 4,
        rows: 3,
        id_base: 0x1000,
    };

    /// A second grid, beside the first, with its own ids.
    const OTHER: CellGrid = CellGrid {
        origin: Vec2::new(200.0, 50.0),
        cell: Vec2::splat(10.0),
        columns: 2,
        rows: 2,
        id_base: 0x2000,
    };

    /// The payload the tests drag: an item name, anchored at `ORIGIN`.
    const ITEM: &str = "plate";
    /// Where the item's footprint starts.
    const ORIGIN: UVec2 = UVec2::new(1, 0);
    /// The item's footprint: a 2×2, so it covers `(1, 0)..=(2, 1)`.
    const SIZE: UVec2 = UVec2::splat(2);

    fn covers(cell: UVec2) -> bool {
        cell.checked_sub(ORIGIN)
            .is_some_and(|offset| offset.cmplt(SIZE).all())
    }

    fn source(cell: UVec2) -> Option<Grip<&'static str>> {
        covers(cell).then_some(Grip {
            payload: ITEM,
            origin: ORIGIN,
        })
    }

    fn centre(grid: &CellGrid, cell: UVec2) -> Vec2 {
        let (at, to) = grid.cell_bounds(cell);
        (at + to) * 0.5
    }

    fn press(pos: Vec2) -> PointerInput {
        PointerInput {
            pos,
            down: true,
            released: false,
        }
    }

    fn release(pos: Vec2) -> PointerInput {
        PointerInput {
            pos,
            down: false,
            released: true,
        }
    }

    /// One frame over [`GRID`], with `accept` as its target.
    fn frame(
        drag: &mut GridDrag<&'static str>,
        ui: &mut UiState,
        pointer: PointerInput,
        accept: bool,
    ) -> (GridResponse, Option<Dropped<&'static str>>) {
        let mut frame = drag.frame(ui, pointer);
        let response = frame.grid(&GRID, source, |_, _| accept);
        (response, frame.finish())
    }

    /// Press on `from`, move to `to`, release on `to`.
    fn drag_between(
        drag: &mut GridDrag<&'static str>,
        ui: &mut UiState,
        from: UVec2,
        to: UVec2,
        accept: bool,
    ) -> (GridResponse, Option<Dropped<&'static str>>) {
        let (_, pressed) = frame(drag, ui, press(centre(&GRID, from)), accept);
        assert_eq!(pressed, None, "a press alone dropped something");
        let (_, moving) = frame(drag, ui, press(centre(&GRID, to)), accept);
        assert_eq!(moving, None, "a drag dropped before release");
        let (response, _) = frame(drag, ui, press(centre(&GRID, to)), accept);
        let (_, dropped) = frame(drag, ui, release(centre(&GRID, to)), accept);
        (response, dropped)
    }

    /// **Every cell's centre and corner hit-test back to it, and its id
    /// round-trips.** The drawing and the hit test are one convention; a grid
    /// whose two disagreed would drag the wrong cell.
    #[test]
    fn the_hit_test_and_the_drawing_agree_about_where_a_cell_is() {
        let cells: Vec<UVec2> = GRID.cells().collect();
        assert_eq!(cells.len(), 12, "a 4x3 grid has twelve cells");
        for cell in cells {
            let (at, _) = GRID.cell_bounds(cell);
            assert_eq!(GRID.cell_at(centre(&GRID, cell)), Some(cell));
            assert_eq!(GRID.cell_at(at + Vec2::splat(0.5)), Some(cell));
            assert_eq!(GRID.cell_of(GRID.id_of(cell)), Some(cell));
            assert_eq!(OTHER.cell_of(GRID.id_of(cell)), None);
        }
        assert_eq!(GRID.cell_at(GRID.origin - Vec2::ONE), None);
        assert_eq!(
            GRID.cell_at(Vec2::new(140.0, 60.0)),
            None,
            "past the last column"
        );
        assert_eq!(GRID.cell_at(Vec2::new(f32::NAN, 55.0)), None);
    }

    /// **A drag from A to B reports `(A, B)`.**
    #[test]
    fn a_drag_from_one_cell_to_another_reports_both() {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let from = UVec2::new(1, 0);
        let to = UVec2::new(2, 2);
        let (_, dropped) = drag_between(&mut drag, &mut ui, from, to, true);
        let dropped = dropped.expect("the drag was not reported");
        assert_eq!(dropped.payload, ITEM);
        assert_eq!(
            dropped.from,
            GridCell {
                grid: GRID.id_base,
                cell: from
            }
        );
        assert_eq!(
            dropped.to.at,
            GridCell {
                grid: GRID.id_base,
                cell: to
            }
        );
        assert!(drag.held().is_none(), "the drag outlived its release");
        assert_eq!(ui.active(), None, "the capture outlived the press");
    }

    /// **A release where the drag began does nothing**, and nor does it light
    /// the cell as a target.
    #[test]
    fn a_release_where_the_drag_began_drops_nothing() {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let cell = UVec2::new(2, 1);
        let (response, dropped) = drag_between(&mut drag, &mut ui, cell, cell, true);
        assert_eq!(dropped, None, "a click was reported as a drop");
        assert_eq!(response.cell(cell).drop, DropFeedback::None);
        assert_eq!(response.cell(cell).state, ButtonState::Pressed);
        assert!(drag.held().is_none());
    }

    /// **A refusing target is drawn as refusing and drops nothing; the same
    /// drag over an accepting one is drawn as accepting.**
    #[test]
    fn a_refused_target_gives_refusing_feedback_and_no_drop() {
        let from = UVec2::new(1, 1);
        let to = UVec2::new(3, 2);

        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let (response, dropped) = drag_between(&mut drag, &mut ui, from, to, false);
        assert_eq!(response.cell(to).drop, DropFeedback::Refusing);
        assert_eq!(response.cell(from).drop, DropFeedback::None);
        assert_eq!(dropped, None, "a refused target took the drop");
        assert!(
            drag.held().is_none(),
            "a refused release did not end the drag"
        );

        // The control: the same drag, accepted.
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let (response, dropped) = drag_between(&mut drag, &mut ui, from, to, true);
        assert_eq!(response.cell(to).drop, DropFeedback::Accepting);
        assert!(dropped.is_some(), "an accepting target dropped nothing");
    }

    /// **The grab offset is kept**: a 2×2 taken by its bottom-right cell and
    /// let go with the pointer on `(3, 2)` lands its origin on `(2, 1)`, and a
    /// grip that would push the origin off the top-left edge is refused
    /// without asking the target.
    #[test]
    fn the_grab_offset_is_kept() {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let grabbed = UVec2::new(2, 1);
        let (_, dropped) = drag_between(&mut drag, &mut ui, grabbed, UVec2::new(3, 2), true);
        let dropped = dropped.expect("the drag was not reported");
        assert_eq!(dropped.to.origin, UVec2::new(2, 1), "the grip moved");

        // Grabbed by its origin, the origin lands under the pointer.
        let (_, dropped) = drag_between(&mut drag, &mut ui, ORIGIN, UVec2::new(0, 2), true);
        assert_eq!(dropped.expect("a drop").to.origin, UVec2::new(0, 2));

        // Off the left edge: refused, and the target is not consulted.
        let mut asked = false;
        let mut frame = drag.frame(&mut ui, press(centre(&GRID, grabbed)));
        frame.grid(&GRID, source, |_, _| true);
        frame.finish();
        let mut frame = drag.frame(&mut ui, release(centre(&GRID, UVec2::new(0, 2))));
        let response = frame.grid(&GRID, source, |_, _| {
            asked = true;
            true
        });
        assert_eq!(frame.finish(), None, "an origin off the grid was dropped");
        assert_eq!(response.cell(UVec2::new(0, 2)).drop, DropFeedback::Refusing);
        assert!(!asked, "the target was asked about an origin at -1");
    }

    /// **A turned payload keeps its grip inside its new footprint.**
    /// **A quarter turn keeps the grabbed cell under the pointer.** Cell
    /// `(column, row)` of a `w × h` footprint becomes `(h − 1 − row, column)`
    /// of the `h × w` one: EW's `1×2` held by its top cell is held by `(1, 0)`
    /// once it lies `2×1`, and four turns come back to where they began.
    /// **A cell may be any rectangle.** A 300×78 one-cell slot — EW's weapon
    /// card — is hit anywhere inside it and nowhere past it, and a grid of
    /// 20×10 cells places and finds each cell by its own width and height; a
    /// cell of no size, or a non-finite one, is over nothing.
    #[test]
    fn a_cell_may_be_any_rectangle() {
        let slot = CellGrid {
            origin: Vec2::new(40.0, 20.0),
            cell: Vec2::new(300.0, 78.0),
            columns: 1,
            rows: 1,
            id_base: 0x3000,
        };
        assert_eq!(
            slot.cell_bounds(UVec2::ZERO),
            (Vec2::new(40.0, 20.0), Vec2::new(340.0, 98.0))
        );
        assert_eq!(slot.cell_at(Vec2::new(339.0, 97.0)), Some(UVec2::ZERO));
        assert_eq!(slot.cell_at(Vec2::new(200.0, 99.0)), None, "below the card");
        assert_eq!(slot.cell_at(Vec2::new(341.0, 30.0)), None, "past its right");

        let wide = CellGrid {
            origin: Vec2::ZERO,
            cell: Vec2::new(20.0, 10.0),
            columns: 4,
            rows: 3,
            id_base: 0x4000,
        };
        for cell in wide.cells() {
            let (min, max) = wide.cell_bounds(cell);
            assert_eq!(max - min, Vec2::new(20.0, 10.0));
            assert_eq!(wide.cell_at((min + max) / 2.0), Some(cell), "{cell}");
        }
        assert_eq!(wide.cell_at(Vec2::new(25.0, 15.0)), Some(UVec2::new(1, 1)));

        for cell in [Vec2::new(20.0, 0.0), Vec2::new(f32::NAN, 10.0)] {
            let degenerate = CellGrid { cell, ..wide };
            assert_eq!(degenerate.cell_at(Vec2::new(5.0, 5.0)), None, "{cell}");
        }
    }

    /// One frame over [`GRID`] ended with [`DragFrame::release`].
    fn release_frame(
        drag: &mut GridDrag<&'static str>,
        ui: &mut UiState,
        pointer: PointerInput,
        accept: bool,
    ) -> Option<Released<&'static str>> {
        let mut frame = drag.frame(ui, pointer);
        frame.grid(&GRID, source, |_, _| accept);
        frame.release()
    }

    /// **An overlapping grid's refusal does not undo another's acceptance.**
    /// EW's optic strip is a one-cell grid drawn over the weapon card's: both
    /// run each frame, and a drop either takes happens — whichever runs first,
    /// and whichever of the two accepts.
    #[test]
    fn a_later_refusal_over_an_overlapping_grid_keeps_the_drop() {
        // Grabbed at its origin, so a one-cell slot can take it.
        let from = ORIGIN;
        let to = UVec2::new(3, 2);
        let (slot_min, _) = GRID.cell_bounds(to);
        let slot = CellGrid {
            origin: slot_min,
            cell: GRID.cell,
            columns: 1,
            rows: 1,
            id_base: 0x5000,
        };
        for (slot_first, slot_accepts) in
            [(true, true), (false, true), (true, false), (false, false)]
        {
            let mut drag = GridDrag::new();
            let mut ui = UiState::new();
            let mut frame = drag.frame(&mut ui, press(centre(&GRID, from)));
            frame.grid(&GRID, source, |_, _| false);
            frame.finish();

            let mut frame = drag.frame(&mut ui, release(centre(&GRID, to)));
            let run_slot = |frame: &mut DragFrame<'_, &'static str>| {
                frame.grid(&slot, |_| None, |_, _| slot_accepts);
            };
            if slot_first {
                run_slot(&mut frame);
                frame.grid(&GRID, source, |_, _| !slot_accepts);
            } else {
                frame.grid(&GRID, source, |_, _| !slot_accepts);
                run_slot(&mut frame);
            }
            let ended = frame.release().expect("the release ends the drag");
            let case = format!("slot first {slot_first}, slot accepts {slot_accepts}");
            assert!(ended.accepted, "{case}: the acceptance was overwritten");
            let accepting = if slot_accepts {
                slot.id_base
            } else {
                GRID.id_base
            };
            assert_eq!(ended.over.map(|over| over.grid), Some(accepting), "{case}");
        }
    }

    /// **Every way a held drag ends is reported by `release`**: dropped,
    /// refused where the item would have landed, let go over no grid, let go
    /// with the origin off the grid's edge, and a click — while a drag that
    /// goes on, or no drag at all, reports nothing.
    #[test]
    fn release_reports_every_ending() {
        let from = ORIGIN + UVec2::new(1, 1);
        let to = UVec2::new(3, 2);
        let begin = |drag: &mut GridDrag<&'static str>, ui: &mut UiState| {
            assert_eq!(
                release_frame(drag, ui, press(centre(&GRID, from)), true),
                None,
                "a drag that has just begun has not ended"
            );
        };
        let expect_target = DropTarget {
            at: GridCell {
                grid: GRID.id_base,
                cell: to,
            },
            origin: to - UVec2::ONE,
        };

        for accept in [true, false] {
            let mut drag = GridDrag::new();
            let mut ui = UiState::new();
            begin(&mut drag, &mut ui);
            let ended = release_frame(&mut drag, &mut ui, release(centre(&GRID, to)), accept)
                .expect("a release ends the drag");
            assert_eq!(ended.payload, ITEM);
            assert_eq!(ended.from.cell, from);
            assert_eq!(ended.over, Some(expect_target.at));
            assert_eq!(
                ended.target,
                Some(expect_target),
                "the refused target is kept"
            );
            assert_eq!(ended.accepted, accept);
        }

        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        begin(&mut drag, &mut ui);
        let outside = release_frame(&mut drag, &mut ui, release(Vec2::new(-50.0, -50.0)), true)
            .expect("let go over nothing still ends it");
        assert_eq!(
            (outside.over, outside.target, outside.accepted),
            (None, None, false)
        );

        // Grabbed one cell in, let go on column 0: the origin would be off the
        // left edge, so there is a cell but no target.
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        begin(&mut drag, &mut ui);
        let edge = release_frame(
            &mut drag,
            &mut ui,
            release(centre(&GRID, UVec2::new(0, 2))),
            true,
        )
        .expect("ended");
        assert!(edge.over.is_some());
        assert_eq!((edge.target, edge.accepted), (None, false));

        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        begin(&mut drag, &mut ui);
        let click = release_frame(&mut drag, &mut ui, release(centre(&GRID, from)), true)
            .expect("a click ends it too");
        assert_eq!(click.over.map(|over| over.cell), Some(from));
        assert_eq!((click.target, click.accepted), (None, false));

        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        assert_eq!(
            release_frame(&mut drag, &mut ui, release(centre(&GRID, to)), true),
            None,
            "nothing was held"
        );
    }

    #[test]
    fn a_quarter_turn_keeps_the_grabbed_cell() {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let mut frame = drag.frame(&mut ui, press(centre(&GRID, ORIGIN)));
        frame.grid(&GRID, source, |_, _| true);
        frame.finish();
        let held = drag.held_mut().expect("the press took hold");
        assert_eq!(held.grab(), UVec2::ZERO);
        assert!(!held.changed());

        held.turn_quarter(UVec2::new(1, 2));
        assert_eq!(
            held.grab(),
            UVec2::new(1, 0),
            "the 1x2's top cell, lying 2x1"
        );
        assert!(held.changed(), "a turn is a change");

        // Taken as a 3×2 now, held by (1, 0): each turn follows the rule, and
        // four turns of alternating sizes come back to where they began.
        held.refit(UVec2::new(3, 2));
        let start = held.grab();
        let mut sizes = [UVec2::new(3, 2), UVec2::new(2, 3)].into_iter().cycle();
        let mut grab = start;
        for _ in 0..4 {
            let size = sizes.next().expect("cycles");
            let expected = UVec2::new(size.y - 1 - grab.y, grab.x);
            held.turn_quarter(size);
            grab = expected;
            assert_eq!(held.grab(), grab);
        }
        assert_eq!(grab, start, "four quarter turns are a whole one");
    }

    /// **A payload changed while held drops where it started**; unchanged,
    /// the same release there is a click. EW turns an item in place: press
    /// it, turn it, let go on the same cell.
    #[test]
    fn a_changed_payload_released_where_it_began_is_a_drop() {
        let cell = ORIGIN;
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        frame(&mut drag, &mut ui, press(centre(&GRID, cell)), true);
        drag.held_mut().expect("held").payload_mut();
        let (response, dropped) = frame(&mut drag, &mut ui, release(centre(&GRID, cell)), true);
        let dropped = dropped.expect("a changed payload let go in place drops");
        assert_eq!(dropped.to.at.cell, cell);
        assert_eq!(dropped.to.origin, ORIGIN);
        assert_eq!(response.cell(cell).drop, DropFeedback::Accepting);

        // The control: the same press and release with nothing changed.
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        frame(&mut drag, &mut ui, press(centre(&GRID, cell)), true);
        let (_, dropped) = frame(&mut drag, &mut ui, release(centre(&GRID, cell)), true);
        assert_eq!(dropped, None, "an unchanged click dropped");
    }

    #[test]
    fn a_refit_grip_stays_inside_the_footprint() {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let mut frame = drag.frame(&mut ui, press(centre(&GRID, UVec2::new(2, 1))));
        frame.grid(&GRID, source, |_, _| true);
        frame.finish();
        let held = drag.held_mut().expect("the press took hold");
        assert_eq!(held.grab(), UVec2::new(1, 1));
        held.refit(UVec2::new(1, 3));
        assert_eq!(held.grab(), UVec2::new(0, 1));
        held.refit(UVec2::new(3, 1));
        assert_eq!(held.grab(), UVec2::new(0, 0));
    }

    /// **A press on one grid and a release on another is one drag**, reported
    /// with each end's grid; a release over neither drops nothing.
    #[test]
    fn a_drag_crosses_from_one_grid_to_another() {
        let run = |to: Vec2| {
            let mut drag = GridDrag::new();
            let mut ui = UiState::new();
            let mut dropped = None;
            for pointer in [press(centre(&GRID, ORIGIN)), press(to), release(to)] {
                let mut frame = drag.frame(&mut ui, pointer);
                frame.grid(&GRID, source, |_, _| true);
                frame.grid(
                    &OTHER,
                    |_| None,
                    |_, target| target.origin == UVec2::new(1, 1),
                );
                dropped = frame.finish();
            }
            (dropped, drag.held().is_some())
        };

        let (dropped, still_held) = run(centre(&OTHER, UVec2::new(1, 1)));
        let dropped = dropped.expect("the cross-grid drag was not reported");
        assert_eq!(dropped.from.grid, GRID.id_base);
        assert_eq!(
            dropped.to.at,
            GridCell {
                grid: OTHER.id_base,
                cell: UVec2::new(1, 1)
            }
        );
        assert!(!still_held);

        // The other grid refuses `(0, 0)`, and nothing is under `(0, 0)` on screen.
        assert_eq!(run(centre(&OTHER, UVec2::ZERO)).0, None);
        let (dropped, still_held) = run(Vec2::ZERO);
        assert_eq!(dropped, None, "a release over nothing dropped");
        assert!(!still_held, "a release over nothing kept the drag");
    }

    /// **A capture cleared mid-drag takes the drag with it**, so a panel torn
    /// down mid-press does not reopen holding an item.
    #[test]
    fn a_cleared_capture_drops_the_drag() {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        frame(&mut drag, &mut ui, press(centre(&GRID, ORIGIN)), true);
        assert!(drag.held().is_some());
        ui.clear();
        let (_, dropped) = frame(
            &mut drag,
            &mut ui,
            release(centre(&GRID, UVec2::new(0, 2))),
            true,
        );
        assert_eq!(dropped, None);
        assert!(drag.held().is_none());
    }

    /// **A press over a cell holding nothing starts no drag**, and a later
    /// release elsewhere drops nothing.
    #[test]
    fn a_press_on_an_empty_cell_takes_hold_of_nothing() {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let (_, dropped) =
            drag_between(&mut drag, &mut ui, UVec2::new(0, 2), UVec2::new(1, 0), true);
        assert_eq!(dropped, None);
    }
}
