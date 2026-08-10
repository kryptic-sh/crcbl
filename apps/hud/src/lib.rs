//! hud — the UI system's living fixture.
//!
//! A game-style HUD with no game behind it: health and mana bars, an ability row
//! with cooldown states, a wave banner and a damage ticker, all of it driven by
//! a scripted ticker on the server and all of it built out of the draw list's
//! primitives — filled rectangles, outlines and text spans.
//!
//! # Why it exists
//!
//! Every other sample proves the engine can host a game. This one proves the
//! engine's **UI** can be dogfooded: it is the fixture the UI system is
//! developed against, and it starts the moment the UI core exists rather than
//! waiting for the styling work that finishes it. See
//! [`docs/plan/sample/04-hud.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/04-hud.md).
//!
//! What is here is that document's **milestone 1**: "P4 skeleton — HUD page with
//! the slice-1 primitives (blocks, spans, text, bars)". What is deliberately not
//! here is everything the document files under P10 — the CSS subset and its
//! stylesheets, the two themes and the runtime switcher, the widget gallery, the
//! UI inspector, the hot-reload showcase and the per-theme golden frames. All of
//! those rest on a styling system that does not exist yet, so building toward
//! them now would be machinery with no consumer. `docs/backlog.md` records what
//! each one is waiting on.
//!
//! # It is a real client/server sample
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 has no exemption for a UI
//! demo, and this sample takes none: the ticker is a [`crcbl::ecs::GameModule`]
//! the authoritative server owns, stepped on the fixed timestep, with a client
//! on the other end of an `InMemoryTransport`. See [`game`].
//!
//! # And it draws no sprite art
//!
//! **Exempt from sample rule 11**, by name, in both the rules and this sample's
//! own doc: its subject *is* the widget system, and a `.crpix` sheet in front of
//! it would be showing something other than what it exists to show. There is no
//! `build.rs` here and no bake step. Rule 4's debug panel still applies, and
//! here it is the dogfood case — see [`app::Hud`]'s `debug_sections`.

pub mod app;
mod args;
pub mod game;
mod gpu;
pub mod menu;
pub mod page;

pub use app::{Hud, HudError, Loop, Summary, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use game::{
    ABILITIES, ABILITY_COUNT, AbilitySpec, AbilityView, DAMAGE_LANES, DAMAGE_LIFETIME,
    DEFAULT_SEED, DEFAULT_TICK_HZ, Damage, DamageView, Game, GameError, HEALTH_FLOOR, HEALTH_MAX,
    HudStats, MANA_MAX, RenderState, WAVE_TICKS,
};
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
