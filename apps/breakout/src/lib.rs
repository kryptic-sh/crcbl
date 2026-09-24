//! Breakout — the first playable Crucible sample.
//!
//! A 2D breakout game: paddle, ball, brick grid, score, lives. Runs
//! client+server over in-memory transport, orthographic camera, spatial audio
//! for bounces.
//!
//! # Two front ends, one loop
//!
//! This is a library because the sample has to be reachable from two places
//! that share nothing else:
//!
//! | Front end | Entry | Outer loop |
//! | --- | --- | --- |
//! | native | `src/main.rs` → [`run`] | `while engine.frame()` inside [`run`] |
//! | browser | `web` (`wasm32` only) | `requestAnimationFrame`, in JS |
//!
//! Stage 10's constraint table (in `docs/notes/browser.md`) is the reason the split
//! exists at all: *"Stage 1 frame loop is a `fn tick(dt)` driven by an outer
//! loop, not a `loop {}` that owns the thread"*. `app::Loop::frame` has always
//! been that `tick`; until P5.8 nothing but [`run`] could call it, and a
//! browser cannot call [`run`].
//!
//! Everything below the front ends is shared verbatim — the same
//! `app::Loop`, the same `Game`, the same render graph. The browser differs in
//! exactly three places, each of them a platform fact rather than a fork:
//!
//! - **start-up is polled**, because `requestDevice` is a promise
//!   (`app::PendingLoop`, driven from the `web` module);
//! - **the clock is the browser's**, because `std::time::Instant::now` panics
//!   on `wasm32-unknown-unknown` (`app::Loop::set_frame_step`);
//! - **the high score lives in OPFS**, because there is no filesystem
//!   (`high_score`).

mod app;
mod args;
mod art;
mod audio;
mod game;
mod gpu;
mod high_score;
mod menu;
mod scene;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{BreakoutError, Loop, PendingLoop, Summary, run};
pub use args::{Invocation, Options, USAGE, parse};
// `Options::board`'s type and the component its chunk file is rows of. Public
// because `Options` is: a field nobody outside this crate can name is one they
// cannot construct an `Options` around.
//
// `BOARD`, `built_in_source` and `register_components` are public for a second
// reader: a tool opens this game's committed board through the engine's
// component registry, and `.scn/` is the engine's format rather than this
// game's. See `scene::built_in_source` and `scene::register_components`.
pub use scene::{BOARD, Board, Brick, built_in_source, register_components};
