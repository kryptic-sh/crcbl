//! Navigation: where an agent on the ground can walk, and by which cells.
//!
//! One module today, [`grid`]: a caller-authored field of walkable and blocked
//! square cells, a shortest four-neighbour route across it, and a bounded
//! flood fill that answers "what is within `n` steps of here". It came from
//! EW, whose AI routes and cover search ran on it before the engine had any
//! navigation at all, and it moved here unchanged in behaviour so that EW could
//! delete its copy by changing imports.
//!
//! # The rung below the navmesh, not a replacement for it
//!
//! `docs/plan/24-navigation.md` is still the plan: a Recast-lineage navmesh
//! baked from physics colliders, tiled and sector-aware, with A\* and a funnel
//! over polygons. None of that is built. The grid is what a game uses before
//! it — a bounded arena, a scene whose walkability a game can sample itself —
//! and it stays useful after, for the small local questions (a cover search, a
//! reach test) a polygon mesh is heavier than.
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
//! * **No grid built from a physics world.** Declined on 2026-09-27
//!   (`docs/backlog.md`, _EW's engine port requests_): which probe decides that
//!   a cell is walkable — its clearance, its step height, its margin — is a
//!   scene's policy, and EW is the only game with one. A caller fills the cell
//!   map and hands it to [`grid::GroundGrid::new`].
//! * **No diagonal steps, no costs, no smoothing.** A route is the cell
//!   centres a four-neighbour walk visits. Cutting corners or weighting terrain
//!   is the navmesh's business, or a caller's.

pub mod grid;
