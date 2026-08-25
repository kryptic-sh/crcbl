//! Puppet — a character, a controller and a camera on a small shadowed map.
//!
//! `docs/plan/sample/09-puppet.md`, **milestone 1**: "static character +
//! controller + camera on the map (pre-anim: proves map + controller path with
//! shadows already on)". A capsule walks a blockout; nothing is animated, and
//! that is the point of building this before the animation is there to hide
//! behind.
//!
//! # What it proves
//!
//! One thing, and it is a path rather than a feature: a **key press becomes a
//! world-space displacement and a swept capsule move**, on a server, with the
//! picture drawn from the result.
//!
//! ```text
//!   shell key ──▶ ActionMap ──▶ Controls ──wire──▶ Intent
//!                                                    │
//!                    camera yaw ──▶ walk_direction ──┤
//!                                                    ▼
//!                                    CharacterController::move_and_slide
//!                                                    │
//!                                    MoveOutcome ────┴──▶ facing, camera, overlay
//! ```
//!
//! The sun turns while it happens, which is [`map::sun`] and the only thing on
//! the map that moves without a key being held: a shadow that never moves is
//! indistinguishable from a dark patch painted on the ground, and "does it read
//! as grounded" is the eyeball test this milestone is for.
//!
//! Every surface on [`map`] exists to make one of the controller's own decisions
//! visible: a slope it walks up and one it refuses, a step it climbs and one it
//! does not. Nothing here reimplements any of it — rule 9 — and nothing here is
//! a special case: the same displacement goes into the same call whichever
//! surface is under the capsule.
//!
//! # The controller does not know which camera is watching, and this is why
//!
//! [`crcbl::phys::CharacterController`] takes a world-space displacement and
//! stores no orientation at all. So **this sample** turns a stick into a
//! direction ([`camera::walk_direction`]) and **this sample** turns the body
//! toward where it went ([`game`]). That seam is deliberate — `docs/backlog.md`
//! records why — and a demo that wanted a yaw inside `crcbl-phys` would be the
//! constraint being violated rather than a feature being missed.
//!
//! # It is a real client/server sample
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 has no exemption for a
//! character demo: the walk is a [`crcbl::ecs::GameModule`] the authoritative
//! server owns, stepped on the fixed timestep, with a client on the other end of
//! an `InMemoryTransport`. The camera is the one thing that is **not** on that
//! side, because it is presentation — and the single number that crosses the
//! wire from it is the yaw the player was looking along when they asked to walk
//! forward.
//!
//! # It walks itself until somebody takes the controls
//!
//! A page that has just loaded has had no input, and a character standing still
//! is the same frame a stopped loop would draw. So the character walks a slow
//! circuit on the spawn pad from the first tick, and the first movement key ends
//! it for good — the arrangement `apps/orbit` and `apps/viewer` both use.
//!
//! # What is not here at milestone 1
//!
//! No clip, no blend tree, no skinning, no jump, no root motion, no socket and
//! no device swapping: those are milestones 2 to 4 and `docs/backlog.md` carries
//! the list. Two things are visible in the picture rather than merely absent
//! from it, and both are named where they are: the slopes are **rounded**,
//! because `crcbl-phys` has no oriented box to make a wedge out of
//! ([`map`] says so), and the character is a capsule with a block on the front
//! of it, because there is no rig to draw yet.
//!
//! # Rule 11 does not apply
//!
//! No `.crpix` art. The subject of this sample is a 3D character on a 3D map,
//! and its overlay is the readout a reviewer checks the picture against —
//! pixel art in front of it would be showing the wrong system.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here. `src/web.rs`
//! is the second front end — compiled only on `wasm32`, which is why it is not
//! linked on a host build — and it is what the demo site's shim drives once per
//! `requestAnimationFrame`.

pub mod app;
mod args;
pub mod camera;
pub mod game;
mod gpu;
pub mod map;
pub mod menu;
pub mod page;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, PendingLoop, Puppet, PuppetError, Summary, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use camera::{Follow, walk_direction};
pub use game::{Controls, DEFAULT_TICK_HZ, Game, GameError, RenderState, Stats};
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
