//! Flappy — the engine's second game.
//!
//! A one-button side-scroller: a bird under constant gravity, a flap impulse, an
//! endless procession of pipe gaps, a score, and instant death on contact.
//! Deliberately the smallest game this engine can ship, and smaller than
//! breakout.
//!
//! # Why it exists
//!
//! Not to be a good game. Breakout proved the engine can host *a* game; this one
//! asks whether it can host a **second** one without the first's shape having
//! leaked into the API. A single consumer cannot tell a design from an accident:
//! every convenience that happens to suit a paddle and a brick grid reads as
//! good design until something else needs the same seam. This project has paid
//! for that lesson twice already — the HAL seam that looked complete until
//! `crcbl-wgpu` implemented it, and the shader that compiled until Dawn read it.
//!
//! So the shape here is different on purpose: continuous forward motion instead
//! of a fixed field, procedural spawning instead of a static grid, one button
//! instead of two axes, instant loss instead of three lives. Any place the
//! engine resists that is a **finding**, recorded in `docs/plan/ROADMAP.md`
//! rather than worked around here. See
//! [`docs/plan/sample/12-flappy.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/12-flappy.md).
//!
//! # What is here so far
//!
//! [`Game`] only: the simulation, running server-authoritatively over an
//! in-memory transport exactly as breakout's does. The renderer, the native
//! binary and the browser entry point arrive in later slices.

mod game;

pub use game::{
    BIRD_RADIUS, BIRD_START, DEFAULT_SEED, DEFAULT_TICK_HZ, FIRST_PIPE_X, FLAP_SPEED,
    GAP_CENTRE_RANGE, GAP_HALF_HEIGHT, GRAVITY, Game, GameError, GameState, PIPE_HALF_WIDTH,
    PIPE_SPACING, PipeView, RenderState, SCROLL_SPEED, WORLD_CEILING, WORLD_FLOOR, gap_centre,
    pipe_x,
};
