//! Bracket — matchmaking, rating and ranked session flow, with no game attached.
//!
//! Players queue, get paired, a match is resolved by a stub, ratings move and
//! the ladder updates. What that isolates is the thing a real game cannot test:
//! matchmaking quality is a property of a **population over time**, not of one
//! session, and a matchmaker that pairs well for four players and badly for four
//! thousand is one nobody has tested. With the match resolved by a stub, a
//! population of any size runs deterministically from a seed.
//!
//! See `docs/plan/sample/16-bracket.md`.

//! # Two front ends, one loop
//!
//! Like every other sample, this is a library because it has to be reachable
//! from two places that share nothing else: `src/main.rs` for the native binary,
//! and `src/web.rs` — compiled only on `wasm32`, which is why it is not linked
//! on a host build — for a browser driven from `requestAnimationFrame`.
//! Everything below them is shared verbatim, which for this sample includes the
//! part that makes it worth publishing: the demo takes **no input at all**, so
//! what a visitor sees is a ladder sorting itself out and nothing they did.

pub mod app;
mod args;
mod gpu;
pub mod menu;
pub mod page;
pub mod queue;
pub mod rating;
pub mod sim;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Bracket, BracketError, Loop, PendingLoop, Summary, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use menu::{MenuKind, Menus};
