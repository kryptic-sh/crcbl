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
///
/// # A grid that scrolls
///
/// A stash taller than its panel shows a [`window`](Self::window) of its
/// cells. Every cell keeps its number — the grid's **content** cell, the one
/// ids, grips, drop targets and [`GridCell`]s all name — and only the window
/// is drawn, from [`origin`](Self::origin). So an item whose top row is
/// scrolled off is grabbed by a row that shows, and its [`Grip::origin`] is
/// still a cell of the grid; and a drag held while the grid scrolls under it
/// reads the new window on the next frame's [`DragFrame::grid`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellGrid {
    /// Where the first cell drawn is: the top-left corner of cell `(0, 0)`,
    /// or of the window's first cell for a grid that scrolls.
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
    /// The cells on screen, for a grid that scrolls; `None` draws them all.
    pub window: Option<GridWindow>,
}

/// The part of a [`CellGrid`] on screen: see _A grid that scrolls_ there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridWindow {
    /// The first cell drawn, at the grid's [`origin`](CellGrid::origin): how
    /// far the grid is scrolled, in cells.
    pub first: UVec2,
    /// How many cells are drawn across and down, from `first`.
    pub size: UVec2,
}

impl CellGrid {
    /// The window's first cell and size: [`window`](Self::window), or the
    /// whole grid.
    fn shown(&self) -> GridWindow {
        self.window.unwrap_or(GridWindow {
            first: UVec2::ZERO,
            size: UVec2::new(self.columns, self.rows),
        })
    }

    /// Where `cell` is drawn: its top-left and bottom-right corners. For a
    /// cell outside the [`window`](Self::window), where it would be drawn if
    /// the window reached it.
    #[must_use]
    pub fn cell_bounds(&self, cell: UVec2) -> (Vec2, Vec2) {
        let from_first = cell.as_ivec2() - self.shown().first.as_ivec2();
        let at = self.origin + from_first.as_vec2() * self.cell;
        (at, at + self.cell)
    }

    /// Which cell `pos` is over, or `None` for a point outside the grid — or
    /// outside its [`window`](Self::window), for a grid that scrolls.
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
        let local = local.floor().as_uvec2();
        let shown = self.shown();
        if local.cmpge(shown.size).any() {
            return None;
        }
        let cell = shown.first.saturating_add(local);
        (cell.x < self.columns && cell.y < self.rows).then_some(cell)
    }

    /// Whether `cell` is a cell of the grid inside its
    /// [`window`](Self::window): one that is drawn.
    #[must_use]
    pub fn shows(&self, cell: UVec2) -> bool {
        let shown = self.shown();
        cell.x < self.columns
            && cell.y < self.rows
            && cell.cmpge(shown.first).all()
            && (cell - shown.first).cmplt(shown.size).all()
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

    /// Every cell, row by row from the top-left, drawn or not.
    pub fn cells(&self) -> impl Iterator<Item = UVec2> {
        let columns = self.columns;
        (0..self.rows).flat_map(move |y| (0..columns).map(move |x| UVec2::new(x, y)))
    }

    /// The cells the [`window`](Self::window) shows, row by row: what a panel
    /// draws.
    pub fn visible_cells(&self) -> impl Iterator<Item = UVec2> + '_ {
        self.cells().filter(|&cell| self.shows(cell))
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
mod tests;
