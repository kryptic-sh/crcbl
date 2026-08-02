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
//! The simulation, and the picture of it: the arena, the player, three enemy
//! kinds, contact damage, hit points, death and restart; XP gems that drop where
//! an enemy died and a "pick 1 of 3" level-up from a small fixed pool; `.crpix`
//! art baked by `build.rs` and drawn through `SpriteRenderer` with
//! `SampleMode::Pixel`; pause, level-up and death menus, the debug panel,
//! fullscreen and focus handling.
//!
//! **Two sheets, and that is the sample's own decision.** Everything numerous —
//! the player, all three enemy kinds and the gems — shares one sheet at one
//! frame size, so the whole field is a single `SpriteRenderer` batch whatever
//! order it is emitted in; only the shot is separate. `src/art.rs` carries the
//! argument, and the scale sub-slice is what measures it.
//!
//! The scale push, the measured budgets and the browser demo are the sub-slice
//! after, and until then the enemy cap is [`DEFAULT_MAX_ENEMIES`] rather than the
//! plan's ten thousand — see that constant for why that is a decision and not an
//! oversight.
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
mod art;
mod game;
mod gpu;
mod menu;

pub use app::{
    DEBUG_OVERLAY_KEY, FULLSCREEN_KEY, HordeError, Loop, MENU_ACTIVATE_KEY, MENU_DOWN_KEY,
    MENU_UP_KEY, PAUSE_KEY, Summary, run,
};
pub use args::{Invocation, Options, USAGE, parse};
pub use art::{ACTOR_HALF_EXTENT, BOLT_HALF_EXTENT, GROUND, Scene, TEXELS_PER_UNIT};
pub use game::{
    ARENA_HALF_HEIGHT, ARENA_HALF_WIDTH, BOLT_DAMAGE, BOLT_LIFE, BOLT_RADIUS, BOLT_SPEED, BoltView,
    DEFAULT_MAX_ENEMIES, DEFAULT_SEED, DEFAULT_TICK_HZ, EnemyKind, EnemyView, FIRE_COOLDOWN,
    FIRE_COOLDOWN_FLOOR, Game, GameError, GameState, MAX_PICKUPS, PLAYER_MAX_HP, PLAYER_RADIUS,
    PLAYER_SPEED, PickupView, RenderState, SEPARATION_SLACK, SEPARATION_STRENGTH,
    SPAWN_INTERVAL_MIN, SPAWN_INTERVAL_START, SPAWN_RAMP_SECONDS, SPAWN_RING, Setup, Stats,
    UPGRADE_CHOICES, Upgrade, VIEW_HALF_HEIGHT, WEAPON_RANGE, XP_RADIUS, clamp_axis,
    clamp_to_arena, hash_unit, max_enemy_radius, separation_query_radius, spawn_interval,
    spawn_jitter, spawn_kind, spawn_offset, upgrade_offer, xp_for_next_level,
};
pub use gpu::{camera_centre, view_half_width};
pub use menu::{MenuAction, MenuKind, Menus};
