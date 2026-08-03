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
//! argument, and [`SceneStats`] is where the game reports the number it
//! produces.
//!
//! Five spatial cues — the gun, an enemy coming apart, a gem, a level and the
//! player's own end — through `crcbl-audio`'s grammar with the listener on the
//! player, which is the first sample whose listener moves; the longest run kept
//! in `~/.config/horde` or the browser's Origin Private File System; and the
//! browser entry point the demo site's shim drives.
//!
//! The default enemy cap is [`DEFAULT_MAX_ENEMIES`] rather than the plan's ten
//! thousand — see that constant for why that is a decision and not an oversight
//! — and `--prefill` is how the measurement gets to ten thousand without waiting
//! for a spawner that would take ten minutes to put them there.
//!
//! # Two front ends, one loop
//!
//! Like breakout, flappy and asteroids, this is a library because the sample has
//! to be reachable from two places that share nothing else: `src/main.rs` for
//! the native binary, and `src/web.rs` — compiled only on `wasm32`, which is why
//! it is not linked here — for a browser driven from `requestAnimationFrame`.
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

pub use app::{HordeError, Loop, PendingLoop, Summary, run};
pub use args::{Invocation, Options, USAGE, parse};
pub use art::{ACTOR_HALF_EXTENT, BOLT_HALF_EXTENT, GROUND, Scene, SceneStats, TEXELS_PER_UNIT};
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
