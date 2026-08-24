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

pub mod queue;
pub mod rating;
pub mod sim;
