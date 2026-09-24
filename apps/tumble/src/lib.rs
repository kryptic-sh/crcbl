//! Tumble — the physics gallery: each rung of the contact solver on the scene
//! built to prove it.
//!
//! `docs/plan/sample/24-tumble.md`, **milestone 7, rung 5 "Bridge"**, from
//! the contact solver's rung table (`docs/notes/simulation.md`), over
//! milestone 6's rung 4 "Bullets", milestone 5's rung 3 "Settle", milestone
//! 4's rung 2 "Tower", milestone 3's rung 1 "Pachinko" and milestone 2's rung
//! 0 "Spin": six rooms — the zero-g T-handle and a box that lands ([`spin`]),
//! the obstacle wall with falling balls, pills and cubes ([`wall`]), a
//! thousand-ball pit ([`pit`]), a column, a pyramid and dominoes ([`tower`]), a
//! point-blank cannon at a thin plate and a brick wall with a fast spinning
//! plank beside a pillar ([`bullets`]), and a gapped Newton's cradle, a plank
//! bridge an anvil snaps, and capsule ragdolls down stairs ([`bridge`]) — with
//! every counter of the six rungs on the page, rung 5's being the cradle's
//! momentum in and out, the bridge's sag, the joint error and the joints
//! broken. Settle has no room of its own: its claim is that every other room
//! comes to rest and sleeps. See [`scene`] for how the rooms share a tick and a
//! hash.
//!
//! # What is not here yet
//!
//! **Milestone 1 was skipped rather than built**: its bullet scene is
//! milestone 6's room now, but the wind tunnel and the golden frame are not
//! built. Milestone 4's Galton board is not built either. Every later rung's
//! scene is `docs/backlog.md`'s to carry, and what each room cannot show yet
//! is on its hint line.
//!
//! # It runs itself
//!
//! The one key it reads — `1` to `6` — picks the room on screen and never
//! reaches the simulation. The rooms start the same way every run, which is
//! what lets the browser gate hold the wasm build's hash to the constant the
//! native test pins.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here.
//! `src/web.rs` is the second front end, compiled only on `wasm32`.

pub mod app;
mod args;
pub mod bridge;
pub mod bullets;
mod gpu;
pub mod menu;
pub mod page;
pub mod pit;
pub mod scene;
pub mod spin;
pub mod stage;
pub mod tower;
pub mod wall;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, PendingLoop, Summary, Tumble, TumbleError, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
pub use scene::{Reading, Scenes, View};
