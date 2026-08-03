//! Asteroids — the engine's third game, and the churn sample.
//!
//! A ship that turns, thrusts and wraps; bullets that sweep; rocks in three
//! sizes that split twice; waves that grow; score, lives, game over, restart.
//!
//! # Why it exists
//!
//! Breakout proved the engine can host *a* game. Flappy proved it can host a
//! **second** without the first's shape having leaked into the API. This one
//! asks a different question: **what happens when entities never stop being
//! created and destroyed?** Breakout spawns its world once. Flappy runs a
//! treadmill at a couple of entities a second. Asteroids fires a bullet every
//! sixth of a second, turns one rock into two, and deals a whole wave at a time
//! — so generational ids, deferred destruction and pool slot recycling all get
//! hammered, and a leak shows up as a number that climbs.
//!
//! It is also the first consumer of the P6 physics slice: the simulation drives
//! thrust and damping through the L1 force pipeline, bullets through segment
//! CCD, and ship-versus-rock through a broadphase sphere overlap — and it is the
//! caller that had to decide what a screen wrap means to a BVH. See
//! [`docs/plan/sample/02-asteroids.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/02-asteroids.md).
//!
//! # What is here, and what is not
//!
//! The simulation, and the picture of it: `.crpix` art baked by `build.rs` and
//! drawn through `SpriteRenderer` with `SampleMode::Pixel`, the ship and the
//! rocks turning through the sprite pass's `rotation`, start / pause / game-over
//! menus, the debug panel, pause, fullscreen and focus handling.
//!
//! **This is the first sample where a drawn thing turns**, so it is also where
//! the question that opened had to be answered: an angle integrated per tick and
//! drawn per frame stutters, and an angle wraps, so the renderer interpolates it
//! the short way round. [`lerp_angle`] carries the argument.
//!
//! Three spatial cues — the engine, the gun, and a rock coming apart — through
//! `crcbl-audio`'s grammar, with the listener at the camera in the middle of the
//! field; a best score in `~/.config/asteroids` or the browser's Origin Private
//! File System; and the browser entry point the demo site's shim drives.
//!
//! # Two front ends, one loop
//!
//! Like breakout and flappy, this is a library because the sample has to be
//! reachable from two places that share nothing else: `src/main.rs` for the
//! native binary, and `src/web.rs` — compiled only on `wasm32`, which is why it
//! is not linked here — for a browser driven from `requestAnimationFrame`.
//! Everything below them is shared verbatim.

mod app;
mod args;
mod art;
mod audio;
mod best;
mod game;
mod gpu;
mod menu;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Asteroids, AsteroidsError, Loop, PendingLoop, Summary, run};
pub use args::{Invocation, Options, USAGE, parse};
pub use game::{
    BULLET_LIFE, BULLET_RADIUS, BULLET_SPEED, BulletView, DEFAULT_SEED, DEFAULT_TICK_HZ,
    FIRE_COOLDOWN, FIRST_WAVE_ROCKS, Game, GameError, GameState, MAX_BULLETS, MAX_WAVE_ROCKS,
    RESPAWN_CLEAR_RADIUS, RESPAWN_DELAY, RESPAWN_MAX_WAIT, RenderState, RockSize, RockView,
    SHIP_DAMPING, SHIP_RADIUS, SHIP_THRUST, SHIP_TURN_RATE, SPLIT_CHILDREN, STARTING_LIVES,
    WORLD_HALF_HEIGHT, WORLD_HALF_WIDTH, hash_unit, heading_vector, lerp_angle, wave_rock_position,
    wave_rock_velocity, wave_rocks, wrap_axis, wrap_position, wrap_to_pi,
};
pub use menu::{FIRE_ID, Fire, MenuKind, Menus, fire_from_id};
