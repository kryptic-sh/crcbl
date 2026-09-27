//! Bounded, deterministic routing over a ground grid of walkable and blocked
//! cells.
//!
//! A [`GroundGrid`] is a [`GroundGridConfig`] — where the grid sits, how big a
//! cell is and how many there are — and one [`GridCell`] per cell, row by row
//! along +X, rows stacked along +Z. It answers two questions about a world
//! position on the ground:
//!
//! * [`GroundGrid::plan`]: the cell centres of a shortest four-neighbour walk to
//!   another position, or why there is none.
//! * [`GroundGrid::reachable_within`]: every walkable cell centre within a step
//!   budget, with how many steps each one took.
//!
//! # The grid is flat
//!
//! Only a position's X and Z pick its cell; its Y is read for finiteness and
//! otherwise ignored, and every cell centre handed back sits at the origin's
//! Y. The grid describes one floor. A caller whose ground is not flat keeps its
//! own heights and uses these answers for X and Z.
//!
//! # Why breadth-first search, and how ties are broken
//!
//! Every step between neighbouring cells costs the same, so a breadth-first
//! search already finds a shortest route: the first time it reaches a cell is
//! over the fewest steps. A\* with a distance heuristic finds a route of the
//! same length while visiting fewer cells, but which of several equally short
//! routes it returns depends on how its priority queue orders equal keys — one
//! more thing two implementations can disagree on. Breadth-first has no such
//! queue. [`MAX_GRID_CELL_COUNT`] bounds the whole grid, so the cells it
//! visits are bounded too.
//!
//! **The tie-break is the neighbour order, +Z, then +X, then -Z, then -X**,
//! over a first-in first-out frontier. A cell's predecessor is the first
//! frontier cell to reach it, so when several shortest routes exist the one
//! returned leans +Z first and +X second: on an open grid it goes all the way
//! +Z before it turns +X. The same grid and the same two positions always
//! return the same route, on every machine, and
//! `tests::identical_grids_give_identical_routes_that_lean_plus_z_then_plus_x`
//! is what holds the order to that.
//!
//! # Validation is at the boundary
//!
//! [`GroundGrid::new`] refuses a non-finite origin, a cell size that is not
//! finite and positive, a zero width or height, a cell count past
//! [`MAX_GRID_CELL_COUNT`] (or past `usize`), and a cell map whose length is
//! not that count — each with its own [`GroundGridConfigError`]. After that
//! every index the queries compute is in range by construction. A query
//! refuses a non-finite position, a position outside the grid, and a start or
//! goal on a blocked cell, with a [`GroundRouteError`].
//!
//! # Cost
//!
//! Each [`plan`](GroundGrid::plan) and
//! [`reachable_within`](GroundGrid::reachable_within) allocates one scratch
//! entry per cell of the grid, and visits each cell at most once. Nothing is
//! kept between calls, so a grid is shared by reference and never mutated by
//! a query.

use glam::DVec3;
use std::collections::VecDeque;

/// The most cells a [`GroundGrid`] may hold.
///
/// Every query allocates scratch space proportional to the cell count, so this
/// is what bounds a query's memory and time as well as the grid's own. A config
/// past it is refused with [`GroundGridConfigError::TooManyCells`] rather than
/// clamped, because a clamped grid would describe a different floor.
pub const MAX_GRID_CELL_COUNT: usize = 262_144;

/// Where a ground grid sits and how it is cut up.
///
/// `origin` is the grid's minimum X/Z corner, and its Y is the height every
/// cell centre is reported at. Cell `(x, z)` covers
/// `[origin.x + x·cell_size_m, origin.x + (x+1)·cell_size_m)` along X and the
/// same along Z, for `x < width` and `z < height`.
///
/// The fields are public and nothing is checked here, so a config can be a
/// `const`; [`GroundGrid::new`] is where it is validated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundGridConfig {
    /// The grid's minimum X/Z corner, and the Y of every cell centre.
    pub origin: DVec3,
    /// The side of one square cell, in metres.
    pub cell_size_m: f64,
    /// Cells along +X.
    pub width: usize,
    /// Cells along +Z.
    pub height: usize,
}

impl GroundGridConfig {
    /// A config from its four fields, unchecked — see [`GroundGrid::new`] for
    /// what is refused.
    #[must_use]
    pub const fn new(origin: DVec3, cell_size_m: f64, width: usize, height: usize) -> Self {
        Self {
            origin,
            cell_size_m,
            width,
            height,
        }
    }

    /// The number of cells, or which rule the config breaks.
    fn cell_count(self) -> Result<usize, GroundGridConfigError> {
        if !self.origin.is_finite() {
            return Err(GroundGridConfigError::InvalidOrigin);
        }
        if !self.cell_size_m.is_finite() || self.cell_size_m <= 0.0 {
            return Err(GroundGridConfigError::InvalidCellSize);
        }
        if self.width == 0 || self.height == 0 {
            return Err(GroundGridConfigError::InvalidDimensions);
        }
        let count = self
            .width
            .checked_mul(self.height)
            .ok_or(GroundGridConfigError::TooManyCells)?;
        if count > MAX_GRID_CELL_COUNT {
            return Err(GroundGridConfigError::TooManyCells);
        }
        Ok(count)
    }
}

/// Whether an agent may stand on one ground-grid cell.
///
/// Authored by the caller: this crate does not decide what is walkable, only
/// how to cross what it is told is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridCell {
    /// An agent may stand here and step through.
    Walkable,
    /// An agent may neither stand here nor step through.
    Blocked,
}

/// Why [`GroundGrid::new`] refused a config or a cell map.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum GroundGridConfigError {
    /// A component of the origin is NaN or infinite.
    #[error("the grid origin is not a finite position")]
    InvalidOrigin,
    /// The cell size is NaN, infinite, zero or negative.
    #[error("the cell size is not a finite positive length")]
    InvalidCellSize,
    /// The width or the height is zero.
    #[error("the grid has a zero width or height")]
    InvalidDimensions,
    /// `width × height` overflows `usize` or exceeds [`MAX_GRID_CELL_COUNT`].
    #[error("the grid has more cells than MAX_GRID_CELL_COUNT allows")]
    TooManyCells,
    /// The cell map does not hold exactly `width × height` cells.
    #[error("the cell map's length is not the grid's width times its height")]
    CellCountMismatch,
}

/// Why a query on a [`GroundGrid`] produced no answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum GroundRouteError {
    /// A position has a NaN or infinite component.
    #[error("a position is not finite")]
    InvalidWorldPosition,
    /// A position's X or Z falls outside the grid.
    #[error("a position is outside the grid")]
    OutsideGrid,
    /// The start or the goal is on a blocked cell, or no walkable route joins
    /// them.
    #[error("no walkable route joins the start and the goal")]
    NoRoute,
}

/// A route across a [`GroundGrid`], as the cell centres to walk to.
#[derive(Clone, Debug, PartialEq)]
pub struct GroundRoute {
    /// The centre of every cell the walk enters, in order, ending with the
    /// goal's. The start cell is not in it — the agent is already there — so a
    /// start and goal in the same cell give an empty list.
    pub waypoints: Vec<DVec3>,
}

/// A validated ground grid: its config and one [`GridCell`] per cell.
#[derive(Clone, Debug, PartialEq)]
pub struct GroundGrid {
    config: GroundGridConfig,
    cells: Vec<GridCell>,
}

impl GroundGrid {
    /// A grid from its config and its cells, row by row along +X with rows
    /// stacked along +Z: cell `(x, z)` is `cells[z * width + x]`.
    ///
    /// # Errors
    ///
    /// The config's first broken rule, checked in [`GroundGridConfigError`]'s
    /// declaration order, then [`GroundGridConfigError::CellCountMismatch`] if
    /// `cells` is not `width × height` long.
    pub fn new(
        config: GroundGridConfig,
        cells: Vec<GridCell>,
    ) -> Result<Self, GroundGridConfigError> {
        let cell_count = config.cell_count()?;
        if cells.len() != cell_count {
            return Err(GroundGridConfigError::CellCountMismatch);
        }
        Ok(Self { config, cells })
    }

    /// A shortest four-neighbour route from the cell holding
    /// `start_world_position` to the cell holding `goal_world_position`.
    ///
    /// Among equally short routes, the one the +Z, +X, -Z, -X neighbour order
    /// reaches first — see the module docs. A start and goal in the same
    /// walkable cell give an empty route.
    ///
    /// # Errors
    ///
    /// [`GroundRouteError::InvalidWorldPosition`] or
    /// [`GroundRouteError::OutsideGrid`] for the start, then for the goal;
    /// [`GroundRouteError::NoRoute`] if either is on a blocked cell or no
    /// walkable route joins them.
    pub fn plan(
        &self,
        start_world_position: DVec3,
        goal_world_position: DVec3,
    ) -> Result<GroundRoute, GroundRouteError> {
        let start = self.world_to_cell(start_world_position)?;
        let goal = self.world_to_cell(goal_world_position)?;
        if self.cells[start] == GridCell::Blocked || self.cells[goal] == GridCell::Blocked {
            return Err(GroundRouteError::NoRoute);
        }
        if start == goal {
            return Ok(GroundRoute {
                waypoints: Vec::new(),
            });
        }

        let mut predecessors = vec![None; self.cells.len()];
        let mut frontier = VecDeque::new();
        predecessors[start] = Some(start);
        frontier.push_back(start);

        while let Some(current) = frontier.pop_front() {
            for neighbor in self.neighbors(current).into_iter().flatten() {
                if self.cells[neighbor] == GridCell::Blocked || predecessors[neighbor].is_some() {
                    continue;
                }
                predecessors[neighbor] = Some(current);
                if neighbor == goal {
                    return Ok(self.build_route(start, goal, &predecessors));
                }
                frontier.push_back(neighbor);
            }
        }

        Err(GroundRouteError::NoRoute)
    }

    /// Every walkable cell centre reachable from `start_world_position` within
    /// `max_steps` four-neighbour steps, with its step count.
    ///
    /// The start cell comes first, at zero steps. After it the list is in
    /// breadth-first order — step counts never decrease — and cells at the same
    /// count are in the order the +Z, +X, -Z, -X neighbour order reached them,
    /// so the list is deterministic. A cell's count is its fewest steps from
    /// the start; no cell appears twice, and none past `max_steps` appears at
    /// all. `max_steps == 0` gives the start alone.
    ///
    /// # Errors
    ///
    /// [`GroundRouteError::InvalidWorldPosition`] or
    /// [`GroundRouteError::OutsideGrid`] for the start;
    /// [`GroundRouteError::NoRoute`] if it is on a blocked cell.
    pub fn reachable_within(
        &self,
        start_world_position: DVec3,
        max_steps: usize,
    ) -> Result<Vec<(DVec3, usize)>, GroundRouteError> {
        let start = self.world_to_cell(start_world_position)?;
        if self.cells[start] == GridCell::Blocked {
            return Err(GroundRouteError::NoRoute);
        }
        let mut steps = vec![None; self.cells.len()];
        let mut frontier = VecDeque::new();
        let mut reached = Vec::new();
        steps[start] = Some(0);
        frontier.push_back(start);
        while let Some(current) = frontier.pop_front() {
            let current_steps = steps[current].expect("a queued cell has its step count");
            reached.push((self.cell_center(current), current_steps));
            if current_steps == max_steps {
                continue;
            }
            for neighbor in self.neighbors(current).into_iter().flatten() {
                if self.cells[neighbor] == GridCell::Walkable && steps[neighbor].is_none() {
                    steps[neighbor] = Some(current_steps + 1);
                    frontier.push_back(neighbor);
                }
            }
        }
        Ok(reached)
    }

    /// The side of one grid cell, in metres — what one step of
    /// [`reachable_within`](Self::reachable_within) is worth.
    #[must_use]
    pub fn cell_size_m(&self) -> f64 {
        self.config.cell_size_m
    }

    /// The index of the cell holding `position`'s X and Z.
    fn world_to_cell(&self, position: DVec3) -> Result<usize, GroundRouteError> {
        if !position.is_finite() {
            return Err(GroundRouteError::InvalidWorldPosition);
        }
        let relative_x = (position.x - self.config.origin.x) / self.config.cell_size_m;
        let relative_z = (position.z - self.config.origin.z) / self.config.cell_size_m;
        // Finite inputs can still overflow here — a position near `f64::MAX`
        // over a small cell — and an infinite offset is certainly off the grid.
        if !relative_x.is_finite() || !relative_z.is_finite() {
            return Err(GroundRouteError::OutsideGrid);
        }
        let x = relative_x.floor();
        let z = relative_z.floor();
        if x < 0.0 || z < 0.0 || x >= self.config.width as f64 || z >= self.config.height as f64 {
            return Err(GroundRouteError::OutsideGrid);
        }
        Ok(z as usize * self.config.width + x as usize)
    }

    /// The in-grid neighbours of `cell`, in the tie-break order the module
    /// docs name: +Z, +X, -Z, -X. `None` is an edge of the grid.
    fn neighbors(&self, cell: usize) -> [Option<usize>; 4] {
        let x = cell % self.config.width;
        let z = cell / self.config.width;
        [
            if z + 1 < self.config.height {
                Some(cell + self.config.width)
            } else {
                None
            },
            if x + 1 < self.config.width {
                Some(cell + 1)
            } else {
                None
            },
            if z > 0 {
                Some(cell - self.config.width)
            } else {
                None
            },
            if x > 0 { Some(cell - 1) } else { None },
        ]
    }

    /// The route from `start` to `goal` read back along `predecessors`.
    fn build_route(
        &self,
        start: usize,
        goal: usize,
        predecessors: &[Option<usize>],
    ) -> GroundRoute {
        let mut cells = Vec::new();
        let mut current = goal;
        while current != start {
            cells.push(current);
            current = predecessors[current].expect("visited route cell has a predecessor");
        }
        cells.reverse();
        GroundRoute {
            waypoints: cells
                .into_iter()
                .map(|cell| self.cell_center(cell))
                .collect(),
        }
    }

    /// The centre of `cell`, at the origin's height.
    fn cell_center(&self, cell: usize) -> DVec3 {
        let x = cell % self.config.width;
        let z = cell / self.config.width;
        DVec3::new(
            self.config.origin.x + (x as f64 + 0.5) * self.config.cell_size_m,
            self.config.origin.y,
            self.config.origin.z + (z as f64 + 0.5) * self.config.cell_size_m,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(width: usize, height: usize, cells: Vec<GridCell>) -> GroundGrid {
        GroundGrid::new(
            GroundGridConfig::new(DVec3::ZERO, 1.0, width, height),
            cells,
        )
        .unwrap()
    }

    #[test]
    fn straight_route_returns_cell_centres_after_the_start_cell() {
        let grid = grid(4, 1, vec![GridCell::Walkable; 4]);

        let route = grid
            .plan(DVec3::new(0.1, 0.0, 0.1), DVec3::new(3.9, 0.0, 0.1))
            .unwrap();

        assert_eq!(route.waypoints.len(), 3);
        assert_eq!(
            route.waypoints,
            vec![
                DVec3::new(1.5, 0.0, 0.5),
                DVec3::new(2.5, 0.0, 0.5),
                DVec3::new(3.5, 0.0, 0.5),
            ]
        );
    }

    #[test]
    fn route_around_a_wall_uses_the_only_gap() {
        let grid = grid(
            5,
            3,
            vec![
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Blocked,
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Blocked,
                GridCell::Walkable,
                GridCell::Walkable,
            ],
        );

        let route = grid
            .plan(DVec3::new(0.1, 0.0, 0.1), DVec3::new(4.1, 0.0, 0.1))
            .unwrap();

        assert_eq!(route.waypoints.len(), 6);
        assert!(route.waypoints.contains(&DVec3::new(2.5, 0.0, 1.5)));
        assert_eq!(route.waypoints.last(), Some(&DVec3::new(4.5, 0.0, 0.5)));
    }

    #[test]
    fn sealed_wall_has_no_route() {
        let grid = grid(
            3,
            2,
            vec![
                GridCell::Walkable,
                GridCell::Blocked,
                GridCell::Walkable,
                GridCell::Walkable,
                GridCell::Blocked,
                GridCell::Walkable,
            ],
        );

        assert_eq!(
            grid.plan(DVec3::new(0.1, 0.0, 0.1), DVec3::new(2.1, 0.0, 0.1)),
            Err(GroundRouteError::NoRoute)
        );
    }

    #[test]
    fn positions_outside_the_grid_are_rejected() {
        let grid = grid(1, 1, vec![GridCell::Walkable]);

        assert_eq!(
            grid.plan(DVec3::new(-0.01, 0.0, 0.5), DVec3::new(0.5, 0.0, 0.5)),
            Err(GroundRouteError::OutsideGrid)
        );
        assert_eq!(
            grid.plan(DVec3::new(0.5, 0.0, 0.5), DVec3::new(1.0, 0.0, 0.5)),
            Err(GroundRouteError::OutsideGrid)
        );
    }

    #[test]
    fn equal_cost_routes_use_the_stable_north_then_east_tie_break() {
        let grid = grid(2, 2, vec![GridCell::Walkable; 4]);

        let first = grid
            .plan(DVec3::new(0.1, 0.0, 0.1), DVec3::new(1.1, 0.0, 1.1))
            .unwrap();
        let second = grid
            .plan(DVec3::new(0.1, 0.0, 0.1), DVec3::new(1.1, 0.0, 1.1))
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first.waypoints,
            vec![DVec3::new(0.5, 0.0, 1.5), DVec3::new(1.5, 0.0, 1.5)]
        );
    }

    #[test]
    fn invalid_and_overflow_dimensions_are_rejected() {
        assert_eq!(
            GroundGrid::new(
                GroundGridConfig::new(DVec3::new(f64::NAN, 0.0, 0.0), 1.0, 1, 1),
                vec![],
            ),
            Err(GroundGridConfigError::InvalidOrigin)
        );
        assert_eq!(
            GroundGrid::new(
                GroundGridConfig::new(DVec3::ZERO, 0.0, 1, 1),
                vec![GridCell::Walkable],
            ),
            Err(GroundGridConfigError::InvalidCellSize)
        );
        assert_eq!(
            GroundGrid::new(GroundGridConfig::new(DVec3::ZERO, 1.0, 0, 1), vec![]),
            Err(GroundGridConfigError::InvalidDimensions)
        );
        assert_eq!(
            GroundGrid::new(
                GroundGridConfig::new(DVec3::ZERO, 1.0, usize::MAX, 2),
                vec![],
            ),
            Err(GroundGridConfigError::TooManyCells)
        );
        assert_eq!(
            GroundGrid::new(GroundGridConfig::new(DVec3::ZERO, 1.0, 2, 2), vec![]),
            Err(GroundGridConfigError::CellCountMismatch)
        );
    }

    #[test]
    fn reachable_cells_stop_at_walls_and_the_step_budget() {
        use GridCell::{Blocked, Walkable};
        // A 4×1 corridor with a wall in the third cell.
        let corridor = grid(4, 1, vec![Walkable, Walkable, Blocked, Walkable]);
        let reached = corridor
            .reachable_within(DVec3::new(0.5, 0.0, 0.5), 5)
            .unwrap();
        assert_eq!(
            reached,
            vec![
                (DVec3::new(0.5, 0.0, 0.5), 0),
                (DVec3::new(1.5, 0.0, 0.5), 1)
            ]
        );
        let open = grid(4, 1, vec![Walkable; 4]);
        let reached = open.reachable_within(DVec3::new(0.5, 0.0, 0.5), 2).unwrap();
        assert_eq!(reached.len(), 3, "the budget stops the fill at two steps");
    }

    // The cases below are the engine's own, beyond what EW's suite held.

    /// **Two grids built apart from the same inputs route identically**, and the
    /// route is the one the documented order picks: on an open 3×3 the walk goes
    /// all the way +Z before it turns +X. Swapping the neighbour order would
    /// still find a shortest route, just a different one — which is exactly
    /// the change this is here to catch.
    #[test]
    fn identical_grids_give_identical_routes_that_lean_plus_z_then_plus_x() {
        let a = grid(3, 3, vec![GridCell::Walkable; 9]);
        let b = grid(3, 3, vec![GridCell::Walkable; 9]);
        let start = DVec3::new(0.5, 0.0, 0.5);
        let goal = DVec3::new(2.5, 0.0, 2.5);

        let from_a = a.plan(start, goal).unwrap();
        let from_b = b.plan(start, goal).unwrap();

        assert_eq!(from_a, from_b);
        assert_eq!(
            from_a.waypoints,
            vec![
                DVec3::new(0.5, 0.0, 1.5),
                DVec3::new(0.5, 0.0, 2.5),
                DVec3::new(1.5, 0.0, 2.5),
                DVec3::new(2.5, 0.0, 2.5),
            ]
        );
        assert_eq!(
            a.reachable_within(start, 4).unwrap(),
            b.reachable_within(start, 4).unwrap()
        );
    }

    /// **The flood fill honours its budget exactly**: on an open 5×5 from the
    /// centre, a budget of two reaches the Manhattan diamond of radius two —
    /// every cell in it, each at its Manhattan distance, and nothing outside.
    #[test]
    fn reachable_within_returns_exactly_the_cells_inside_the_budget() {
        let open = grid(5, 5, vec![GridCell::Walkable; 25]);
        let centre = DVec3::new(2.5, 0.0, 2.5);
        let budget = 2;

        let reached = open.reachable_within(centre, budget).unwrap();

        let manhattan = |p: DVec3| ((p.x - centre.x).abs() + (p.z - centre.z).abs()) as usize;
        let inside = (0..5)
            .flat_map(|z: usize| (0..5_usize).map(move |x| (x, z)))
            .filter(|&(x, z)| x.abs_diff(2) + z.abs_diff(2) <= budget)
            .count();
        assert_eq!(reached.len(), inside);
        for &(cell, steps) in &reached {
            assert!(
                steps <= budget,
                "{cell} at {steps} steps is past the budget"
            );
            assert_eq!(steps, manhattan(cell), "{cell} is not at its fewest steps");
        }
        let mut distinct: Vec<_> = reached
            .iter()
            .map(|(cell, _)| (cell.x.to_bits(), cell.z.to_bits()))
            .collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), reached.len(), "a cell was reported twice");

        assert_eq!(
            open.reachable_within(centre, 0).unwrap(),
            vec![(centre, 0)],
            "a zero budget is the start alone"
        );
    }

    /// **The start comes first and the counts never go down**, walls or not.
    #[test]
    fn reachable_within_lists_the_start_first_in_breadth_first_order() {
        use GridCell::{Blocked, Walkable};
        #[rustfmt::skip]
        let cells = vec![
            Walkable, Walkable, Walkable, Walkable,
            Walkable, Blocked,  Blocked,  Walkable,
            Walkable, Walkable, Walkable, Walkable,
        ];
        let grid = grid(4, 3, cells);
        let start = DVec3::new(1.5, 0.0, 0.5);

        let reached = grid.reachable_within(start, usize::MAX).unwrap();

        assert_eq!(reached.first(), Some(&(start, 0)));
        assert!(
            reached.windows(2).all(|pair| pair[0].1 <= pair[1].1),
            "step counts went down: {reached:?}"
        );
        assert_eq!(reached.len(), 10, "every walkable cell is reachable");
    }

    #[test]
    fn a_start_and_goal_in_one_cell_is_an_empty_route() {
        let grid = grid(2, 2, vec![GridCell::Walkable; 4]);

        let route = grid
            .plan(DVec3::new(1.1, 0.0, 1.1), DVec3::new(1.9, 7.0, 1.9))
            .unwrap();

        assert!(route.waypoints.is_empty());
    }

    #[test]
    fn a_blocked_start_or_goal_has_no_route() {
        use GridCell::{Blocked, Walkable};
        let grid = grid(3, 1, vec![Blocked, Walkable, Blocked]);
        let middle = DVec3::new(1.5, 0.0, 0.5);

        assert_eq!(
            grid.plan(DVec3::new(0.5, 0.0, 0.5), middle),
            Err(GroundRouteError::NoRoute)
        );
        assert_eq!(
            grid.plan(middle, DVec3::new(2.5, 0.0, 0.5)),
            Err(GroundRouteError::NoRoute)
        );
        assert_eq!(
            grid.reachable_within(DVec3::new(0.5, 0.0, 0.5), 3),
            Err(GroundRouteError::NoRoute)
        );
    }

    /// A goal walled into its own pocket, with open ground all round the start.
    #[test]
    fn an_enclosed_goal_is_unreachable() {
        use GridCell::{Blocked, Walkable};
        #[rustfmt::skip]
        let cells = vec![
            Walkable, Walkable, Walkable, Walkable, Walkable,
            Walkable, Walkable, Walkable, Blocked,  Walkable,
            Walkable, Walkable, Blocked,  Walkable, Blocked,
            Walkable, Walkable, Walkable, Blocked,  Walkable,
        ];
        let grid = grid(5, 4, cells);

        assert_eq!(
            grid.plan(DVec3::new(0.5, 0.0, 0.5), DVec3::new(3.5, 0.0, 2.5)),
            Err(GroundRouteError::NoRoute)
        );
    }

    #[test]
    fn non_finite_and_overflowing_positions_are_rejected() {
        let grid = GroundGrid::new(
            GroundGridConfig::new(DVec3::new(-1.0, 0.0, -1.0), 0.5, 4, 4),
            vec![GridCell::Walkable; 16],
        )
        .unwrap();
        let inside = DVec3::ZERO;

        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                grid.plan(DVec3::new(bad, 0.0, 0.0), inside),
                Err(GroundRouteError::InvalidWorldPosition)
            );
            assert_eq!(
                grid.plan(inside, DVec3::new(0.0, bad, 0.0)),
                Err(GroundRouteError::InvalidWorldPosition)
            );
            assert_eq!(
                grid.reachable_within(DVec3::new(0.0, 0.0, bad), 1),
                Err(GroundRouteError::InvalidWorldPosition)
            );
        }
        // Finite, but its offset from the origin over the cell size is not.
        assert_eq!(
            grid.plan(DVec3::new(f64::MAX, 0.0, 0.0), inside),
            Err(GroundRouteError::OutsideGrid)
        );
    }

    #[test]
    fn every_non_finite_or_non_positive_cell_size_is_rejected() {
        for size in [f64::NAN, f64::INFINITY, -1.0, -0.0] {
            assert_eq!(
                GroundGrid::new(
                    GroundGridConfig::new(DVec3::ZERO, size, 1, 1),
                    vec![GridCell::Walkable],
                ),
                Err(GroundGridConfigError::InvalidCellSize),
                "{size}"
            );
        }
        assert_eq!(
            GroundGrid::new(
                GroundGridConfig::new(DVec3::new(0.0, f64::INFINITY, 0.0), 1.0, 1, 1),
                vec![GridCell::Walkable],
            ),
            Err(GroundGridConfigError::InvalidOrigin)
        );
    }

    /// The cap is inclusive: a grid of exactly [`MAX_GRID_CELL_COUNT`] cells is
    /// accepted and one row more is not.
    #[test]
    fn the_cell_cap_admits_its_own_size_and_nothing_past_it() {
        let width = 1024;
        let height = MAX_GRID_CELL_COUNT / width;
        assert_eq!(width * height, MAX_GRID_CELL_COUNT);

        let at_cap = GroundGrid::new(
            GroundGridConfig::new(DVec3::ZERO, 1.0, width, height),
            vec![GridCell::Walkable; MAX_GRID_CELL_COUNT],
        );
        assert!(at_cap.is_ok());
        assert_eq!(
            GroundGrid::new(
                GroundGridConfig::new(DVec3::ZERO, 1.0, width, height + 1),
                vec![GridCell::Walkable; MAX_GRID_CELL_COUNT + width],
            ),
            Err(GroundGridConfigError::TooManyCells)
        );
    }

    #[test]
    fn cell_size_is_what_the_config_said() {
        let grid = GroundGrid::new(
            GroundGridConfig::new(DVec3::ZERO, 0.25, 2, 2),
            vec![GridCell::Walkable; 4],
        )
        .unwrap();

        assert_eq!(grid.cell_size_m(), 0.25);
    }
}
