//! Tide — the engine's water acceptance fixture.
//!
//! A gallery of four scenes — open sea, coast, valley and courtyard — each built
//! to show a kind of water, switched on a key, a pause row or a page button.
//! **Not a game**: the water is the content.
//! [`docs/plan/sample/21-tide.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/21-tide.md)
//! is the charter, and its non-goals are a hard cap.
//!
//! # What is here: milestone 1, the courtyard pool, still
//!
//! The rung of `docs/plan/55-water.md` that has landed is a **still** body, and
//! the courtyard is the scene built on it: a tiled pool with a deep end and a
//! shallow end under a coping, drawn through `crcbl::render::ForwardRenderer`'s
//! `set_water` with the surface refracting, absorbing, fading at its edges and
//! reflecting the sky — [`scene`] lays it out, and its header says what each
//! part is for. Beside it:
//!
//! * **The four-scene switch** — [`scene::Scene`] — with the other three drawn as
//!   empty rooms whose placard names the milestone that fills each.
//! * **Four medium presets** — [`medium::Preset`] — clear pool, lake, pond and
//!   swamp, each a point on one bio-optical model whose sources that module's
//!   header cites.
//! * **Three cameras** — [`menu::CameraMode`] — the fixed pose the goldens are
//!   taken from, an orbit on the fixed step, and the free flyer.
//! * **The water's cost per pass** — [`gpu::WaterCost`] — `water-copy` and
//!   `water` off `PassTimers`, on the debug panel, in the headless summary and
//!   in the `[HUD]` heartbeat a browser gate reads.
//! * The path report and forcing flags rule 12 asks for, the debug panel rule 4
//!   asks for, and `tests/golden.rs`: goldens with relational claims in front of
//!   them, because a wrong blue picture is a plausible blue picture.
//!
//! The sun is **fixed** — [`scene::sun`] says why it does not borrow
//! `apps/sundial`'s clock.
//!
//! # No `World`, no system, no server — for this milestone
//!
//! Milestone 1 has no game state: the water is still and nothing floats on it,
//! so there is nothing for a server to own. That is **not** the charter's
//! exemption — the charter says tide is not exempt from sample rule 2, because
//! floating bodies are game state — and **the server loop arrives with
//! milestone 2**, whose pontoon crates are the first state it runs. Until then
//! this follows `apps/sundial`'s no-`World` shape. Rule 11 is exempted by the
//! charter on lantern's ground.
//!
//! # What is not here, and where it is written down
//!
//! Waves, the query and floating (milestone 2); the open sea, valley and coast
//! (3, 4, 5); planar reflection (6); underwater (7); the ripple grid (8) and the
//! fountain (9). Each is a rung of `docs/plan/55-water.md` that has not landed,
//! and the charter's milestone list is where each is scoped.
//!
//! # One library, three front ends
//!
//! `src/main.rs` is argv and an exit code; `tests/golden.rs` renders the same
//! courtyard the binary does; `src/web.rs` is the browser's, compiled only on
//! `wasm32`, and carries the three knobs as exports because natively they are
//! keys and a phone has none.

mod app;
mod args;
mod gpu;
pub mod knobs;
pub mod medium;
pub mod menu;
pub mod scene;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, PendingLoop, Summary, Tide, TideError, run, start, with_shell};
pub use args::{DEFAULT_TICK_HZ, Invocation, Options, USAGE, parse};
pub use gpu::{Gpu, GpuError, Paths, WATER_PASSES, WaterCost};
pub use knobs::Knobs;
pub use menu::{CameraMode, Menus, TideAction, action_for, menus, pause_menu};
