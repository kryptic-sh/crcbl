//! Sparks — the VFX fixture: stock effects, a hostile one, and the budget that
//! holds it.
//!
//! `docs/plan/sample/10-sparks.md`, **milestone 1**: a gallery of effects on a
//! small 3D stage, with the profiler readout that says what each one is costing
//! against what it was allowed.
//!
//! # What it proves
//!
//! ```text
//!   EffectDesc ──▶ ParticleSystem::step ──▶ Live { position, rotation, size, color }
//!                        │                              │
//!            pcg3d(seed, index, stream)                 ▼
//!                        │                  ForwardRenderer::set_instance
//!                        │                              │
//!                  a fixed seed and                     ▼
//!                  a fixed step replay           cull, draw generation,
//!                                                the ordinary forward frame
//! ```
//!
//! Two things, and both are paths rather than features:
//!
//! * **A simulated particle becomes an instance**, and rides the stage 3
//!   GPU-driven pipeline that was already there —
//!   `docs/plan/20-particles.md`'s mesh particles, which "inject transforms into
//!   the stage 3 instance path … for free". There is no particle shader in this
//!   sample and no pass of its own.
//! * **A hostile effect is held at its share.** `docs/plan/sample/10-sparks.md`
//!   asks for a "deliberately hostile effect (max spam)" that "clamps to its
//!   pool share and the panel shows it — never a frame-rate cliff", and
//!   [`effects::spam`] is that effect, on the page, with the refusal counter
//!   beside it.
//!
//! # Colour is quantised, and that is a finding rather than a shortcut
//!
//! The instance path carries a mesh, a **material row** and a transform, and no
//! per-instance tint. So colour over lifetime reaches the screen as
//! [`effects::PALETTE_STEPS`] baked material rows per effect, with each particle
//! drawn through the row nearest the colour the simulation gave it.
//! `crate::effects` argues it and `docs/backlog.md` carries what changing it
//! would cost. It is deliberately not fixed here:
//! `docs/plan/sample/10-sparks.md`'s hard cap is that the gallery exercises what
//! topic 20 ships rather than smuggling engine features in behind it.
//!
//! # What is not here yet
//!
//! Everything above milestone 1, and it is a long list because this is the
//! first slice: no billboards, flipbooks, soft particles, ribbons or
//! depth collision; no sorting; no RON effect assets and so no hot reload; no
//! workbench, no sliders and no curve or gradient widgets; and no GPU
//! simulation — the step runs on the CPU, which is the staging
//! `docs/plan/20-particles.md` asks for and not its destination.
//! `docs/backlog.md` carries each of them with what it would take.
//!
//! # Rule 11 does not apply
//!
//! No `.crpix` art, and `docs/plan/sample/10-sparks.md` grants the exemption in
//! as many words: a particle's texture comes from topic 20's own authoring path,
//! and there is no such path in this slice — these particles are meshes.
//!
//! # It runs itself
//!
//! Nothing here reads a key. `crate::show` says why, and the browser gate
//! depends on it: the count it watches rise and fall does so without anything
//! reaching the page.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here.
//! `src/web.rs` is the second front end — compiled only on `wasm32`, which is
//! why it is not linked on a host build — and it is what the demo site's shim
//! drives once per `requestAnimationFrame`.

pub mod app;
mod args;
pub mod effects;
mod gpu;
pub mod menu;
pub mod page;
pub mod show;
pub mod stage;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, PendingLoop, Sparks, SparksError, Summary, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
pub use show::{Reading, Show};
