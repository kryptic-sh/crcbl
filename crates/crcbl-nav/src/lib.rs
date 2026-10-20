//! Navigation: where an agent on the ground can walk, and by which cells.
//!
//! Two grids, both from EW, whose AI ran on each before it moved here:
//!
//! * [`grid`]: a caller-authored field of walkable and blocked square cells on
//!   one floor, a shortest four-neighbour route across it, and a bounded flood
//!   fill that answers "what is within `n` steps of here".
//! * [`spans`]: the same two questions over each column's walkable floors, as
//!   a caller's probe of the collision world found them, so routes climb
//!   stairs, cross slopes and pass under floors above, kept a body's radius
//!   from walls and drops.
//!
//! And [`follow`], for walking a route either grid planned: which waypoint to
//! steer toward, so a walk turns where its route turns, and whether the walk
//! has stalled.
//!
//! # The rung below the navmesh, not a replacement for it
//!
//! `docs/plan/24-navigation.md` is still the plan: a Recast-lineage navmesh
//! baked from physics colliders, tiled and sector-aware, with A\* and a funnel
//! over polygons. None of that is built. The grids are what a game uses
//! before it — a bounded arena, a scene whose walkability a game can sample
//! itself — and they stay useful after, for the small local questions (a cover
//! search, a reach test) a polygon mesh is heavier than.
//!
//! # Determinism
//!
//! Every answer here is a function of its inputs and nothing else: no hash
//! map, no clock, no randomness, no thread. The same grid asked the same
//! question returns the same route, cell for cell, on every machine — which is
//! what lets a server decide a path and a replay reproduce it. [`grid`]'s docs
//! name the tie-break that makes that true when several routes are equally
//! short.
//!
//! # What is not here
//!
//! * **No probe of a physics world.** Declined on 2026-09-27
//!   (`docs/backlog.md`, _EW's engine port requests_): which probe decides that
//!   a cell is walkable — its clearance, its step height, its margin — is a
//!   scene's policy, and EW is the only game with one. A caller fills the cell
//!   map and hands it to [`grid::GroundGrid::new`], or hands its probe to
//!   [`spans::SpanGrid::build`].
//! * **No smoothing, and no costs on [`grid`].** A route is the cell centres a
//!   walk visits; [`grid`]'s steps are four-neighbour and equal, [`spans`]'s
//!   eight-neighbour and measured. Cutting corners or weighting terrain is the
//!   navmesh's business, or a caller's.

pub mod follow;
pub mod grid;
pub mod spans;
