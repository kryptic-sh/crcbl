//! A height-aware route grid: each column's walkable floors kept as spans, so
//! routes climb stairs, cross sloped ground and pass under floors above.
//!
//! A [`SpanGrid`] is a [`SpanGridConfig`] — where the region sits, how big a
//! cell is and how many there are — a [`NavAgent`], the body routes are for,
//! and the floors a caller's probe found in each column. It answers the same
//! two questions [`GroundGrid`](crate::grid::GroundGrid) does, in the same
//! shapes, but a position's Y now picks which floor of its column it stands on:
//!
//! * [`SpanGrid::plan`]: the floor positions of a shortest walk to another
//!   position, or why there is none.
//! * [`SpanGrid::reachable_within`]: every floor within a step budget, with how
//!   many steps each one took.
//!
//! # The rung toward the navmesh
//!
//! This is the voxel stage of `docs/plan/24-navigation.md` in miniature: a probe
//! of the collision world supplies each column's floors, the agent's limits are
//! data, and clearance is eroded by the agent's radius when the grid is built
//! rather than when it is asked. It came from EW, built along those lines and
//! proven by its AI in play before it moved here. It covers one rectangular
//! region; tiles are not done.
//!
//! # The probe is the caller's
//!
//! [`SpanGrid::build`] takes the probe as a closure, so this crate stays free
//! of physics: which hits count as a floor and how headroom is judged are a
//! scene's policy (declined as an engine probe on 2026-09-27, `docs/backlog.md`,
//! _EW's engine port requests_). A probe casting rays at cell centres should
//! know one trap: a ray exactly on the edge two boxes share can report the
//! side face rather than the top, losing the floor, so geometry authored on
//! the same lattice as the cell centres wants the rays nudged off it.
//!
//! # How a floor is kept, and how floors link
//!
//! A probed floor is kept when its normal is no steeper than
//! [`NavAgent::min_floor_normal_y`], the probe found headroom above it, and
//! every column whose cell comes within [`NavAgent::radius_m`] of its cell's
//! centre holds a probed floor within [`NavAgent::max_climb_m`] of it. That
//! last rule is the erosion: it keeps routes through cell centres a body's
//! radius from walls and drops alike.
//!
//! A floor links to **every** floor within a climb in each of its eight
//! neighbouring columns, not only the nearest in height — linking just the
//! nearest drops stair routes where a landing and a step share a column. A
//! diagonal link also needs both columns it cuts past to hold such a floor.
//!
//! # Determinism, and how ties are broken
//!
//! Routes are A\* over link lengths with the straight-line distance as the
//! estimate. The open set is ordered by estimate and then by span index, which
//! runs row by row along +X from the origin and lowest floor first within a
//! column, and
//! the search state lives in ordered maps; so the same grid asked the same
//! question returns the same route on every machine.
//! [`reachable_within`](SpanGrid::reachable_within) is breadth-first over the
//! links in the order +Z, +X, -Z, -X, then the diagonals.
//!
//! # Cost
//!
//! [`SpanGrid::build`] probes each column once and checks each floor against
//! the columns within the agent's radius, which [`MAX_EROSION_REACH_CELLS`]
//! bounds. Queries keep state only for the spans they touch, so a query near
//! its start stays cheap on a grid of a million spans.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};

use glam::{DVec2, DVec3};

use crate::grid::{GroundRoute, GroundRouteError};

/// The most columns a [`SpanGrid`] may cover.
///
/// The build calls the probe once per column and keeps an index entry per
/// column, so this bounds both. Past it a config is refused with
/// [`SpanGridError::TooManyColumns`] rather than clamped, because a clamped
/// grid would describe a different region. It is far above
/// [`MAX_GRID_CELL_COUNT`](crate::grid::MAX_GRID_CELL_COUNT) because a span
/// grid's queries do not allocate per column the way a ground grid's do.
pub const MAX_SPAN_COLUMN_COUNT: usize = 1 << 22;

/// The farthest the erosion looks, in cells, from a floor's column.
///
/// Every kept floor is checked against every column within the agent's
/// radius, so the build's work grows with the square of the radius over the
/// cell size. A radius that reaches past this is refused with
/// [`SpanGridError::RadiusTooWide`]: a cell that small against the body is
/// finer than routing it needs.
pub const MAX_EROSION_REACH_CELLS: usize = 64;

/// The body a grid is built for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavAgent {
    /// How far routes keep from walls and drops, in metres.
    pub radius_m: f64,
    /// The headroom a floor needs above it, in metres.
    pub height_m: f64,
    /// The tallest rise or drop between neighbouring floors walked without a
    /// jump, in metres.
    pub max_climb_m: f64,
    /// The steepest floor still walked, as its normal's Y: the cosine of the
    /// slope, not the angle.
    pub min_floor_normal_y: f64,
}

#[cfg(feature = "phys")]
impl NavAgent {
    /// The body a character controller moves under: its capsule's radius,
    /// its full height, its step offset as the climb, and its ground slope
    /// limit.
    #[must_use]
    pub fn from_character(config: &crcbl_phys::character::CharacterConfig) -> Self {
        Self {
            radius_m: config.radius,
            height_m: 2.0 * (config.half_height + config.radius),
            max_climb_m: config.step_offset,
            min_floor_normal_y: config.min_ground_normal_y,
        }
    }
}

/// The region a span grid covers.
///
/// Cell `(x, z)` covers `[origin.x + x·cell_size_m, origin.x + (x+1)·cell_size_m)`
/// along X and the same from `origin.y` along Z, for `x < width` and
/// `z < height`. The fields are public and nothing is checked here;
/// [`SpanGrid::build`] is where they are validated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpanGridConfig {
    /// The region's minimum corner: its X, and its Z as the vector's `y`.
    pub origin: DVec2,
    /// The side of one square cell, in metres.
    pub cell_size_m: f64,
    /// Cells along +X.
    pub width: usize,
    /// Cells along +Z.
    pub height: usize,
}

impl SpanGridConfig {
    /// The X and Z of cell (`column`, `row`)'s centre, which is where
    /// [`SpanGrid::build`] probes it.
    #[must_use]
    pub fn cell_center(&self, column: usize, row: usize) -> DVec2 {
        self.origin + (DVec2::new(column as f64, row as f64) + 0.5) * self.cell_size_m
    }

    /// The number of columns, or which rule the config breaks.
    fn column_count(self) -> Result<usize, SpanGridError> {
        if !self.origin.is_finite() {
            return Err(SpanGridError::InvalidOrigin);
        }
        if !self.cell_size_m.is_finite() || self.cell_size_m <= 0.0 {
            return Err(SpanGridError::InvalidCellSize);
        }
        if self.width == 0 || self.height == 0 {
            return Err(SpanGridError::InvalidDimensions);
        }
        self.width
            .checked_mul(self.height)
            .filter(|&count| count <= MAX_SPAN_COLUMN_COUNT)
            .ok_or(SpanGridError::TooManyColumns)
    }
}

/// One floor a probe found in a column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbedFloor {
    /// The floor's height.
    pub y: f64,
    /// The floor's normal's Y, for the slope limit.
    pub normal_y: f64,
    /// Whether the agent's height fits above the floor. How that is judged is
    /// the probe's policy; EW's checks straight above the floor at the cell's
    /// centre, and across the whole cell from a climb above the floor, so a
    /// wall thinner than a cell still blocks the cells it crosses while a
    /// step's edge does not.
    pub headroom_clear: bool,
}

/// Why [`SpanGrid::build`] refused a config, an agent or a probe's answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SpanGridError {
    /// A component of the origin is NaN or infinite.
    #[error("the grid origin is not a finite position")]
    InvalidOrigin,
    /// The cell size is NaN, infinite, zero or negative.
    #[error("the cell size is not a finite positive length")]
    InvalidCellSize,
    /// The width or the height is zero.
    #[error("the grid has a zero width or height")]
    InvalidDimensions,
    /// `width × height` overflows `usize` or exceeds [`MAX_SPAN_COLUMN_COUNT`].
    #[error("the grid has more columns than MAX_SPAN_COLUMN_COUNT allows")]
    TooManyColumns,
    /// A limit of the agent is NaN or infinite, its radius or climb negative,
    /// its height not positive, or its floor normal outside `-1..=1`.
    #[error("the agent's limits are not finite lengths and a normal's Y")]
    InvalidAgent,
    /// The agent's radius reaches more than [`MAX_EROSION_REACH_CELLS`] cells.
    #[error("the agent's radius spans more cells than MAX_EROSION_REACH_CELLS")]
    RadiusTooWide,
    /// The probe reported a floor whose height or normal is NaN or infinite.
    #[error("the probe reported a floor that is not finite")]
    InvalidFloor,
}

/// Every walkable floor in a region, a few per column.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanGrid {
    config: SpanGridConfig,
    agent: NavAgent,
    /// Where each column's floors start in `floors`, with one past the end.
    column_start: Vec<usize>,
    /// Each column's kept floor heights, lowest first.
    floors: Vec<f64>,
}

/// A span: its index in [`SpanGrid::floors`].
type SpanId = usize;

impl SpanGrid {
    /// Builds the grid from `probe`, which reports the floors in the column at
    /// the X and Z it is given, a cell's centre. A floor is kept as the module
    /// docs say.
    ///
    /// # Errors
    ///
    /// The config's first broken rule, in [`SpanGridError`]'s declaration
    /// order, then [`SpanGridError::InvalidAgent`],
    /// [`SpanGridError::RadiusTooWide`], and [`SpanGridError::InvalidFloor`]
    /// for the first non-finite floor the probe reports.
    pub fn build(
        config: SpanGridConfig,
        agent: NavAgent,
        mut probe: impl FnMut(DVec2) -> Vec<ProbedFloor>,
    ) -> Result<Self, SpanGridError> {
        let column_count = config.column_count()?;
        let finite = [
            agent.radius_m,
            agent.height_m,
            agent.max_climb_m,
            agent.min_floor_normal_y,
        ]
        .iter()
        .all(|value| value.is_finite());
        if !finite
            || agent.radius_m < 0.0
            || agent.height_m <= 0.0
            || agent.max_climb_m < 0.0
            || !(-1.0..=1.0).contains(&agent.min_floor_normal_y)
        {
            return Err(SpanGridError::InvalidAgent);
        }
        // Every column whose cell comes within the radius of a cell's centre,
        // in cells: the nearest point of a cell `d` columns over is `d - ½`
        // cells from the centre.
        let reach = agent.radius_m / config.cell_size_m;
        if reach > MAX_EROSION_REACH_CELLS as f64 {
            return Err(SpanGridError::RadiusTooWide);
        }
        // Bounded by the check above; truncation is the point.
        #[allow(clippy::cast_possible_truncation)]
        let steps = (reach + 0.5).ceil() as isize;
        let offsets: Vec<(isize, isize)> = (-steps..=steps)
            .flat_map(|dz| (-steps..=steps).map(move |dx| (dx, dz)))
            .filter(|&(dx, dz)| {
                let gap = DVec2::new(dx.unsigned_abs() as f64, dz.unsigned_abs() as f64) - 0.5;
                gap.max(DVec2::ZERO).length() < reach
            })
            .collect();

        let mut probed: Vec<Vec<f64>> = Vec::with_capacity(column_count);
        for row in 0..config.height {
            for column in 0..config.width {
                let mut floors = Vec::new();
                for floor in probe(config.cell_center(column, row)) {
                    if !floor.y.is_finite() || !floor.normal_y.is_finite() {
                        return Err(SpanGridError::InvalidFloor);
                    }
                    if floor.headroom_clear && floor.normal_y >= agent.min_floor_normal_y {
                        floors.push(floor.y);
                    }
                }
                floors.sort_by(f64::total_cmp);
                probed.push(floors);
            }
        }

        let mut column_start = Vec::with_capacity(column_count + 1);
        let mut floors = Vec::new();
        for row in 0..config.height {
            for column in 0..config.width {
                column_start.push(floors.len());
                for &y in &probed[row * config.width + column] {
                    let clear = offsets.iter().all(|&(dx, dz)| {
                        let (Some(x), Some(z)) = (
                            column.checked_add_signed(dx).filter(|&x| x < config.width),
                            row.checked_add_signed(dz).filter(|&z| z < config.height),
                        ) else {
                            // Past the region's edge is not known to be
                            // walkable, so it erodes like a drop.
                            return false;
                        };
                        probed[z * config.width + x]
                            .iter()
                            .any(|&other| (other - y).abs() <= agent.max_climb_m)
                    });
                    if clear {
                        floors.push(y);
                    }
                }
            }
        }
        column_start.push(floors.len());
        Ok(Self {
            config,
            agent,
            column_start,
            floors,
        })
    }

    /// The region the grid covers.
    #[must_use]
    pub fn config(&self) -> SpanGridConfig {
        self.config
    }

    /// The body the grid was built for.
    #[must_use]
    pub fn agent(&self) -> NavAgent {
        self.agent
    }

    /// The side of one cell, in metres — what one straight step of
    /// [`reachable_within`](Self::reachable_within) is worth.
    #[must_use]
    pub fn cell_size_m(&self) -> f64 {
        self.config.cell_size_m
    }

    /// How many walkable floors the grid holds.
    #[must_use]
    pub fn span_count(&self) -> usize {
        self.floors.len()
    }

    /// The kept floors of the column holding `position`'s X and Z, lowest
    /// first; empty outside the region or for a position that is not finite.
    #[must_use]
    pub fn floors_at(&self, position: DVec3) -> &[f64] {
        self.column_at(position)
            .map_or(&[], |column| &self.floors[self.spans_of(column)])
    }

    /// A shortest walk from the floor under `start_world_position` to the
    /// floor under `goal_world_position`, as the feet position at each span's
    /// cell centre after the start's, ending with the goal's — the shape
    /// [`GroundGrid::plan`](crate::grid::GroundGrid::plan) returns. A start and
    /// goal on the same span give an empty route.
    ///
    /// The floor under a position is the highest in its column at or below
    /// it by up to a climb, or above it by up to a climb.
    ///
    /// # Errors
    ///
    /// [`GroundRouteError::InvalidWorldPosition`] or
    /// [`GroundRouteError::OutsideGrid`] for the start, then for the goal;
    /// [`GroundRouteError::NoRoute`] if either stands on no kept floor or no
    /// walk joins them.
    pub fn plan(
        &self,
        start_world_position: DVec3,
        goal_world_position: DVec3,
    ) -> Result<GroundRoute, GroundRouteError> {
        let from = self.span_at(start_world_position)?;
        let to = self.span_at(goal_world_position)?;
        let (Some(from), Some(to)) = (from, to) else {
            return Err(GroundRouteError::NoRoute);
        };
        let target = self.feet_of(to);
        // Each span reached: its cost so far and the span before it.
        let mut reached: BTreeMap<SpanId, (f64, SpanId)> = BTreeMap::from([(from, (0.0, from))]);
        let mut open = BinaryHeap::from([Open {
            estimate: self.feet_of(from).distance(target),
            span: from,
        }]);
        while let Some(Open { span, .. }) = open.pop() {
            if span == to {
                let mut waypoints = Vec::new();
                let mut at = to;
                while at != from {
                    waypoints.push(self.feet_of(at));
                    at = reached[&at].1;
                }
                waypoints.reverse();
                return Ok(GroundRoute { waypoints });
            }
            let here = self.feet_of(span);
            let so_far = reached[&span].0;
            for next in self.neighbours(span) {
                let there = self.feet_of(next);
                let through = so_far + here.distance(there);
                if reached.get(&next).is_none_or(|&(cost, _)| through < cost) {
                    reached.insert(next, (through, span));
                    open.push(Open {
                        estimate: through + there.distance(target),
                        span: next,
                    });
                }
            }
        }
        Err(GroundRouteError::NoRoute)
    }

    /// Every span within `max_steps` links of the floor under
    /// `start_world_position`, as its feet position and its fewest steps
    /// there, the start first and the rest in breadth-first order (the link
    /// order the module docs name), the same every time. `max_steps == 0`
    /// gives the start alone.
    ///
    /// # Errors
    ///
    /// [`GroundRouteError::InvalidWorldPosition`] or
    /// [`GroundRouteError::OutsideGrid`] for the start;
    /// [`GroundRouteError::NoRoute`] if it stands on no kept floor.
    pub fn reachable_within(
        &self,
        start_world_position: DVec3,
        max_steps: usize,
    ) -> Result<Vec<(DVec3, usize)>, GroundRouteError> {
        let from = self
            .span_at(start_world_position)?
            .ok_or(GroundRouteError::NoRoute)?;
        let mut steps = BTreeMap::from([(from, 0)]);
        let mut frontier = VecDeque::from([from]);
        let mut reached = Vec::new();
        while let Some(span) = frontier.pop_front() {
            let here = steps[&span];
            reached.push((self.feet_of(span), here));
            if here == max_steps {
                continue;
            }
            for next in self.neighbours(span) {
                steps.entry(next).or_insert_with(|| {
                    frontier.push_back(next);
                    here + 1
                });
            }
        }
        Ok(reached)
    }

    /// The column holding `position`'s X and Z, if the region does.
    fn column_at(&self, position: DVec3) -> Option<usize> {
        let local =
            (DVec2::new(position.x, position.z) - self.config.origin) / self.config.cell_size_m;
        if !local.is_finite() || local.x < 0.0 || local.y < 0.0 {
            return None;
        }
        let (column, row) = (local.x.floor(), local.y.floor());
        if column >= self.config.width as f64 || row >= self.config.height as f64 {
            return None;
        }
        // In range by the checks above; truncation is the point.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(row as usize * self.config.width + column as usize)
    }

    /// The span a body with its feet at `position` stands on, or `None` for a
    /// column with no floor within a climb of it.
    fn span_at(&self, position: DVec3) -> Result<Option<SpanId>, GroundRouteError> {
        if !position.is_finite() {
            return Err(GroundRouteError::InvalidWorldPosition);
        }
        let column = self
            .column_at(position)
            .ok_or(GroundRouteError::OutsideGrid)?;
        Ok(self
            .spans_of(column)
            .rev()
            .find(|&span| (position.y - self.floors[span]).abs() <= self.agent.max_climb_m))
    }

    /// The spans of `column`.
    fn spans_of(&self, column: usize) -> std::ops::Range<SpanId> {
        self.column_start[column]..self.column_start[column + 1]
    }

    /// The column `span` is in: the last whose range starts at or before it,
    /// which is never an empty column, since the next starts at the same
    /// index.
    fn column_of(&self, span: SpanId) -> usize {
        self.column_start.partition_point(|&start| start <= span) - 1
    }

    /// The feet position on `span`: its cell's centre at its floor's height.
    fn feet_of(&self, span: SpanId) -> DVec3 {
        let column = self.column_of(span);
        let at = self
            .config
            .cell_center(column % self.config.width, column / self.config.width);
        DVec3::new(at.x, self.floors[span], at.y)
    }

    /// The spans a body steps to from `span`, in the link order the module
    /// docs name.
    fn neighbours(&self, span: SpanId) -> Vec<SpanId> {
        let column = self.column_of(span);
        let (x, z) = (column % self.config.width, column / self.config.width);
        let y = self.floors[span];
        let step = |dx: isize, dz: isize| -> Vec<SpanId> {
            let (Some(nx), Some(nz)) = (
                x.checked_add_signed(dx).filter(|&v| v < self.config.width),
                z.checked_add_signed(dz).filter(|&v| v < self.config.height),
            ) else {
                return Vec::new();
            };
            self.spans_of(nz * self.config.width + nx)
                .filter(|&other| (self.floors[other] - y).abs() <= self.agent.max_climb_m)
                .collect()
        };
        let mut out = Vec::new();
        for (dx, dz) in [(0, 1), (1, 0), (0, -1), (-1, 0)] {
            out.extend(step(dx, dz));
        }
        for (dx, dz) in [(1, 1), (1, -1), (-1, -1), (-1, 1)] {
            if !step(dx, 0).is_empty() && !step(0, dz).is_empty() {
                out.extend(step(dx, dz));
            }
        }
        out
    }
}

/// An open span ordered for a min-heap by estimate, ties by span, so a plan
/// is the same every time.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Open {
    estimate: f64,
    span: SpanId,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimate
            .total_cmp(&self.estimate)
            .then_with(|| other.span.cmp(&self.span))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
#[path = "spans/tests.rs"]
mod tests;
