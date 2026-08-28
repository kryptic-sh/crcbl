//! options — the settings acceptance test.
//!
//! A settings screen with nothing behind it: the player moves a fader, presses
//! `SAVE`, and the value is still there the next time the sample starts. See
//! [`docs/plan/sample/20-options.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/20-options.md).
//!
//! # Why it exists
//!
//! The settings machinery has been built and exercised by everything except a
//! player. `crcbl_store`'s layered TOML stack reads and writes, it resolves a
//! platform config directory natively and OPFS in a browser, `crcbl::settings`
//! has a reader and a writer for every key anything reads, and
//! `crcbl settings set` writes one from a command line. What had never happened
//! is an **application** writing a setting — so `SettingsStack::save_platform`
//! shipped with no caller that a player could reach.
//!
//! This sample is that caller.
//!
//! # What is here, and what the document still wants
//!
//! Its milestone 1: the six `[engine.audio]` bus gains, edited on a screen,
//! written to the player's own settings file, read back at the next start, and —
//! since [`audio`] landed — audible while they move, so a fader is a gain stage
//! rather than a key nothing reads. The browser build sample rule 7 requires is
//! here too, and the browser gate drives the whole round trip through it.
//!
//! What is **not** here is the video and graphics halves of the catalogue —
//! milestones 2 and 3 — which `docs/backlog.md` carries.
//!
//! # It steps no simulation and loads no art
//!
//! **Exempt from sample rules 2 and 10** — no game state, no `World`, no
//! `GameModule` — and **from rule 11**, on the ground `apps/hud` claims it: the
//! subject is a screen, and a sprite sheet in front of it would be showing
//! something else. Its own plan doc claims all three by name.
//!
//! # Two front ends, one loop
//!
//! Like every other sample, this is a library because it has to be reachable
//! from two places that share nothing else: `src/main.rs` for the native binary,
//! and `src/web.rs` for a browser entry point driven from
//! `requestAnimationFrame` — behind `cfg(target_arch = "wasm32")`, so it is not
//! in these docs unless they were built for that target.
//! Everything below them is shared verbatim.

pub mod app;
mod args;
pub mod audio;
pub mod gpu;
pub mod menu;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{
    APP_NAME, HEARTBEAT_TICKS, Loop, Options, OptionsError, PendingLoop, SaveState, Screen, Store,
    Summary, run, start, with_shell,
};
pub use args::{DEFAULT_TICK_HZ, Invocation, USAGE, parse};
pub use audio::Audio;
pub use menu::{Action, MenuKind, Menus};
