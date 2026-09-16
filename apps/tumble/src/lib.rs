//! Tumble — the physics gallery: each rung of the contact solver on the scene
//! built to prove it.
//!
//! `docs/plan/sample/24-tumble.md`, **milestone 2, rung 0 "Spin"**, from
//! `docs/plan/36-contact-solver.md`'s rung table: a zero-g T-handle and a box
//! dropped flat, with angular momentum and energy drift, step time and the
//! determinism hash on the page. See [`scene`] for both scenes and what each
//! can and cannot show before rung 1.
//!
//! # What is not here yet
//!
//! **Milestone 1 was skipped rather than built**: the ball pit's spawn fountain,
//! the bullet scene and the wind tunnel, the scene switch, and the golden
//! frame. Milestone 2 is here because rung 0 is what landed. Every later rung's
//! scene is `docs/backlog.md`'s to carry.
//!
//! # It runs itself
//!
//! Nothing here reads a key. The scenes start the same way every run, which is
//! what lets the browser gate hold the wasm build's hash to the constant the
//! native test pins.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here.
//! `src/web.rs` is the second front end, compiled only on `wasm32`.

pub mod app;
mod args;
mod gpu;
pub mod menu;
pub mod page;
pub mod scene;
pub mod stage;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, PendingLoop, Summary, Tumble, TumbleError, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
pub use scene::{Reading, Scenes};
