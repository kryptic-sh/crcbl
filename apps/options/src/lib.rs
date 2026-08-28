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
//! Its milestone 1, minus the sound: the six `[engine.audio]` bus gains, edited
//! on a screen, written to the player's own settings file and read back at the
//! next start. What is **not** here is anything to hear those buses with, the
//! video half of the catalogue, and the browser build sample rule 7 requires.
//! `docs/backlog.md` carries all three, and [`app`]'s module docs say what the
//! audio one costs.
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
//! and — once the web half lands — a browser entry point driven from
//! `requestAnimationFrame`. Everything below them is shared verbatim.

pub mod app;
mod args;
pub mod gpu;
pub mod menu;

pub use app::{
    APP_NAME, Loop, Options, OptionsError, PendingLoop, SaveState, Screen, Store, Summary, run,
    start, with_shell,
};
pub use args::{DEFAULT_TICK_HZ, Invocation, USAGE, parse};
pub use menu::{Action, MenuKind, Menus};
