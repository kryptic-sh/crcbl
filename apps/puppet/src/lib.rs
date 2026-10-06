//! Puppet — a character, a controller and a camera on a small shadowed map.
//!
//! `docs/plan/sample/09-puppet.md`, **milestone 1 and the first half of
//! milestone 2**: a character walks a blockout under a controller, and a rig is
//! posed by a locomotion blend the controller's own measured speed drives.
//!
//! # What it proves
//!
//! Two things, and both are paths rather than features. A **key press becomes a
//! world-space displacement and a swept capsule move**, on a server, with the
//! picture drawn from the result — and the **speed that move actually achieved
//! selects the pose**, blended between clips rather than switched between them.
//!
//! ```text
//!   shell key ──▶ ActionMap ──▶ Controls ──wire──▶ Intent
//!                                                    │
//!                    camera yaw ──▶ OrbitCamera::walk_direction ──┤
//!                                                    ▼
//!                                    CharacterController::move_and_slide
//!                                                    │
//!                                    MoveOutcome ────┼──▶ facing, camera, overlay
//!                                                    │
//!                                     measured speed ┴──▶ BlendSpace1d
//!                                                            │
//!                                            Pose ─▶ Palette ┴─▶ skinning
//! ```
//!
//! # The rig is code, and the blend is the engine's
//!
//! [`rig`] authors a greybox humanoid — nine joints, five boxes, an idle stance
//! and a walk cycle — with no asset on disk and no glTF parse; it says there why.
//! [`anim`] drives it, and the blending itself is
//! [`crcbl::anim::blend`]'s rather than this sample's. The character is drawn
//! through the engine's **skinning dispatch**, one range per limb, so the demo
//! is the acceptance test that path is meant to be rather than a picture that
//! happens to move.
//!
//! The sun turns while it happens, which is [`map::Map::sun`] and the only thing on
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
//! # The map is content, not code
//!
//! Those surfaces live in `assets/scenes/blockout.scn/` — a `.scn/` directory of
//! RON chunk files, read through [`crcbl::scene::scn`] — and [`map`] is its
//! loader. `--scene <DIR>` walks a different one, and the committed directory is
//! compiled in so a browser (which has no filesystem) and a golden (which must
//! not depend on a working directory) both get the same map. The component a
//! chunk row is — a greybox primitive with the collider that is the same
//! surface — is puppet's own type rather than an engine one, and [`map`] says
//! why.
//!
//! # The controller does not know which camera is watching, and this is why
//!
//! [`crcbl::phys::CharacterController`] takes a world-space displacement and
//! stores no orientation at all. So **this sample** turns a stick into a
//! direction ([`crcbl::render::OrbitCamera::walk_direction`]) and **this sample** turns the body
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
//! # Two heartbeats, on two clocks
//!
//! `[HUD]` is the simulation's, logged from the tick — where the character is
//! and what the controller decided. `[POSE]` is the client's, logged from the
//! frame — what the blend did with the speed. They are separate because their
//! clocks are, and `Puppet::report_pose` in [`app`] argues it where the second one
//! is written.
//!
//! # It walks itself until somebody takes the controls
//!
//! A page that has just loaded has had no input, and a character standing still
//! is the same frame a stopped loop would draw. So the character walks a slow
//! circuit on the spawn pad from the first tick, and the first movement key ends
//! it for good — the arrangement `apps/orbit` and `apps/viewer` both use.
//!
//! # Two beacons hum behind the mounds
//!
//! The one sound: [`audio`] stands a beacon behind each mound and hears it from
//! the camera through the same [`crcbl::client::ClientQueryWorld`] the boom is
//! swept through, so a mound between the two muffles it — earth more than
//! brush — and walking round comes clear. That is the cue grammar's rule 5,
//! [`crcbl::occlusion`], in a sample. A headless run plays nothing.
//!
//! # What is not here yet
//!
//! No state machine, no jump, no run, no root motion, no animation events, no
//! socket and no device swapping: those are the rest of milestone 2 and
//! milestones 3 and 4, and `docs/backlog.md` carries the list. Two things are
//! visible in the picture rather than merely absent from it, and both are named
//! where they are: the slopes are **rounded**, because `crcbl-phys` has no
//! oriented box to make a wedge out of ([`map`] says so), and the character is
//! five boxes, because [`rig`] authors a blockout humanoid rather than importing
//! one.
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

pub mod anim;
pub mod app;
mod args;
pub mod audio;
pub mod camera;
pub mod game;
mod gpu;
pub mod map;
pub mod menu;
pub mod page;
pub mod rig;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, PendingLoop, Puppet, PuppetError, Summary, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use camera::Follow;
pub use game::{Controls, DEFAULT_TICK_HZ, Game, GameError, RenderState, Stats};
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
