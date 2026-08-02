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
//! This is the **simulation** slice. The game is complete and playable headless;
//! the native binary opens a window and draws the field as untextured
//! placeholder quads through the UI pass, plus the HUD and the debug panel.
//! There is no art, no `.crpix` sheet, no menus, no audio and no browser entry
//! point — each arrives with its own sub-slice, and each is a thing the samples
//! before this one already show how to do.

mod app;
mod args;
mod game;
mod gpu;

pub use app::{AsteroidsError, Loop, Summary, run};
pub use args::{Invocation, Options, USAGE, parse};
pub use game::{
    BULLET_LIFE, BULLET_RADIUS, BULLET_SPEED, BulletView, DEFAULT_SEED, DEFAULT_TICK_HZ,
    FIRE_COOLDOWN, FIRST_WAVE_ROCKS, Game, GameError, GameState, MAX_BULLETS, MAX_WAVE_ROCKS,
    RESPAWN_CLEAR_RADIUS, RESPAWN_DELAY, RESPAWN_MAX_WAIT, RenderState, RockSize, RockView,
    SHIP_DAMPING, SHIP_RADIUS, SHIP_THRUST, SHIP_TURN_RATE, SPLIT_CHILDREN, STARTING_LIVES,
    WORLD_HALF_HEIGHT, WORLD_HALF_WIDTH, hash_unit, heading_vector, wave_rock_position,
    wave_rock_velocity, wave_rocks, wrap_axis, wrap_position,
};
