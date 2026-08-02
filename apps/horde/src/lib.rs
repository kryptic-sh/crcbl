//! Horde — the engine's fourth game, and the scale sample.
//!
//! A survivors-lite: one arena, one player, a gun that aims itself, and a crowd
//! that gets bigger until it kills you.
//!
//! # Why it exists
//!
//! Breakout proved the engine can host *a* game. Flappy proved it can host a
//! **second** without the first's shape having leaked into the API. Asteroids
//! asked what happens when entities never stop being created and destroyed. This
//! one asks a different question again: **what does one tick cost per live
//! body?** Asteroids churns hard and holds fifty things; horde holds a thousand
//! and then wants ten, and every one of them steers, queries the broadphase and
//! is drawn.
//!
//! It is the first consumer of the dynamic broadphase at a scale where the
//! broadphase is the point: `N` overlap queries a tick for separation, one for
//! contact damage, one for aiming, and a swept sphere per bolt. See
//! [`docs/plan/sample/03-horde.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/03-horde.md).
//!
//! # What is here, and what is not
//!
//! **The core loop only**: the arena, the player, three enemy kinds, contact
//! damage, hit points, death and restart — drawn as untextured quads through the
//! UI pass, with the debug panel on. The `.crpix` art, the XP pickups and the
//! level-up screen are the next sub-slice. The scale push, the measured budgets
//! and the browser demo are the one after that, and until then the enemy cap is
//! [`DEFAULT_MAX_ENEMIES`] rather than the plan's ten thousand — see that
//! constant for why that is a decision and not an oversight.
//!
//! # Two front ends, one loop
//!
//! Like breakout, flappy and asteroids, this is a library because the sample has
//! to be reachable from two places that share nothing else: `src/main.rs` for
//! the native binary, and a browser entry point driven from
//! `requestAnimationFrame`. Only the first exists yet; the package's shape is
//! here from the start so the second is a module rather than a restructure.

mod app;
mod args;
mod game;
mod gpu;

pub use app::{
    DEBUG_OVERLAY_KEY, FULLSCREEN_KEY, HordeError, Loop, MAX_DRAWN_ENEMIES, PAUSE_KEY, Summary,
    draw_field, run,
};
pub use args::{Invocation, Options, USAGE, parse};
pub use game::{
    ARENA_HALF_HEIGHT, ARENA_HALF_WIDTH, BOLT_DAMAGE, BOLT_LIFE, BOLT_RADIUS, BOLT_SPEED, BoltView,
    DEFAULT_MAX_ENEMIES, DEFAULT_SEED, DEFAULT_TICK_HZ, EnemyKind, EnemyView, FIRE_COOLDOWN, Game,
    GameError, GameState, PLAYER_MAX_HP, PLAYER_RADIUS, PLAYER_SPEED, RenderState,
    SEPARATION_SLACK, SEPARATION_STRENGTH, SPAWN_INTERVAL_MIN, SPAWN_INTERVAL_START,
    SPAWN_RAMP_SECONDS, SPAWN_RING, Setup, VIEW_HALF_HEIGHT, WEAPON_RANGE, clamp_axis,
    clamp_to_arena, hash_unit, max_enemy_radius, separation_query_radius, spawn_interval,
    spawn_jitter, spawn_kind, spawn_offset,
};
pub use gpu::{camera_centre, pixels_per_unit, view_half_width, world_to_screen};
