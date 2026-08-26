//! Breach — a first-person firing range, on the same controller `apps/puppet`
//! walks.
//!
//! `docs/plan/sample/11-breach.md`, **milestone 0**: the web slice's firing
//! range *and* its bot practice map, single player, running natively and in a
//! browser from one build.
//!
//! # What it proves
//!
//! One thing above all: **[`crcbl::phys::CharacterController`] is
//! camera-agnostic.** `apps/puppet` drives it from a third-person orbit camera;
//! this sample drives the same controller from a first-person camera, and
//! neither the controller nor `crcbl-phys` gained a line on breach's behalf.
//! [`camera`] is where that argument lives, and it names
//! `apps/puppet/src/camera.rs` as its other half — one demo saying "the
//! controller does not know which camera is watching" is a comment, and two
//! demos driving it from cameras that share no code is evidence.
//!
//! And one thing beside it: **a hitscan weapon is a ray into the same world the
//! capsule sweeps against.** [`crcbl::phys::PhysicsWorld::cast_ray`] is the
//! whole of the pistol, and nothing in this sample intersects anything itself.
//!
//! ```text
//!   shell key ──▶ ActionMap ──▶ Controls ──wire──▶ Intent
//!                                                    │
//!    mouse / arrows ──▶ Eye { yaw, pitch } ──────────┤
//!                                                    ▼
//!                            walk_direction ──▶ CharacterController::move_and_slide
//!                                                    │
//!                            forward ──▶ Ray ──▶ PhysicsWorld::cast_ray
//!                                                    │
//!                                    Aim ────────────┴──▶ crosshair, score, plates
//! ```
//!
//! # Two maps
//!
//! [`MapChoice`] picks between them, `--map` sets it on a command line and
//! `__crcbl_breach_map` sets it from a page — the shape `apps/horde`'s
//! `--prefill` already has. The browser's default is the range, so
//! `/demos/breach/` is the page it has always been and `?map=practice` is what
//! asks for the other one.
//!
//! # The range
//!
//! [`map`] is a greybox room: a floor, four walls, a ceiling, a firing line the
//! player cannot walk past, and three lanes at different distances with one
//! target plate each. Every surface is a [`crcbl::greybox`] primitive over a
//! constant that the colliders are written from too, so what looks shootable is
//! shootable.
//!
//! The firing line is not a rule in the game code: it is a kerb over the
//! controller's own
//! [`step_offset`](crcbl::phys::CharacterConfig::step_offset), so the
//! controller refuses it. Nothing in [`game`] checks where the player is
//! standing.
//!
//! # It is a real client/server sample
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 has no exemption for a
//! shooter: the walk and the shot are a [`crcbl::ecs::GameModule`] the
//! authoritative server owns, stepped on the fixed timestep, with a client on
//! the other end of an `InMemoryTransport`. The camera is the one thing that is
//! **not** on that side, because it is presentation — and what crosses the wire
//! from it is the pair of angles the player was looking along when they walked
//! and when they pulled the trigger.
//!
//! # It shoots itself until somebody steps up to the line
//!
//! A page that has just loaded has had no input, and a player standing still in
//! an empty room is the same frame a stopped loop would draw. So the range
//! swings onto each lane in turn and fires from the first tick; the first
//! movement key or trigger pull ends that for good and resets the range — every
//! plate up, the score at zero, the view squared up down the near lane. [`game`]
//! carries the argument, and [`camera::Eye::point_at`] is how the view follows
//! it.
//!
//! # Rule 12, on the one target where the fallbacks are not hypothetical
//!
//! [`Paths`] reads the three selectors off the device and puts them on the
//! debug panel, the `[HUD]` heartbeat and the summary line. A browser has no
//! mesh stage and no ray query, so a visitor's frame goes through
//! `IndirectPerBatch` and `LightingPath::Rasterised` by construction — which is
//! why milestone 0 is built before the native game rather than after it.
//!
//! # The practice map
//!
//! [`map::practice`] is the second greybox room: a pillar and two crates for
//! cover, and three bots on **authored patrol routes**. Each walks its list of
//! waypoints through the same [`crcbl::phys::CharacterController`], notices the
//! player with the same [`crcbl::phys::PhysicsWorld::cast_ray`] the pistol is,
//! and shoots back on a fixed cadence with that same pistol. The player has
//! health and respawns; the bots go down when they are shot and come back.
//!
//! **There is no navmesh and no pathfinding.** `docs/plan/24-navigation.md` is a
//! post-MVP subsystem whose own text names `arena`'s bots as its forcing
//! function rather than breach's, and [`bots`] is where the argument lives —
//! along with why the sighting ray is cast from the player's end.
//!
//! # What is not here yet
//!
//! **Milestone 0 is two maps and nothing else.** No weapon but the one hitscan
//! pistol — no ballistics, no penetration, no armour, no ADS, no recoil, no
//! reload and no viewmodel; no inventory, no rounds, no economy and no
//! networking beyond the in-memory loopback every sample has. Those are
//! milestones 1 onward, and `docs/backlog.md` carries the list with what each
//! would take.
//!
//! Two things are absent from the picture rather than merely from the feature
//! list, and both are deliberate: **the player is invisible**, because a
//! first-person slice with no viewmodel has nothing to draw of them and a
//! borrowed rig would be a second character system to maintain — and on the
//! practice map that goes further than cosmetics, since a player with no body
//! is a player the bots walk through; and **the rooms are lit by lamps rather
//! than by a sun**, because they have ceilings — [`map::house_light`] says so
//! where the light is built.
//!
//! # Rule 11 does not apply
//!
//! No `.crpix` art. The subject of this sample is a 3D room seen from inside
//! it, and its overlay is a crosshair and the readout a reviewer checks the
//! picture against — pixel art in front of it would be showing the wrong
//! system. `docs/plan/sample/11-breach.md` does ask for it, and names what for:
//! the grid inventory's item icons, the buy menu, the killfeed and the
//! scoreboard. Milestone 0 has none of those.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here. `src/web.rs`
//! is the second front end — compiled only on `wasm32`, which is why it is not
//! linked on a host build — and it is what the demo site's shim drives once per
//! `requestAnimationFrame`.

pub mod app;
mod args;
pub mod bots;
pub mod camera;
pub mod game;
mod gpu;
pub mod map;
pub mod menu;
pub mod page;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Breach, BreachError, Loop, PendingLoop, Summary, run, start, with_shell};
pub use args::request_map;
pub use args::{Invocation, Options, USAGE, parse};
pub use camera::{Eye, forward, walk_direction};
pub use game::{
    Aim, ArenaStats, Controls, DEFAULT_TICK_HZ, Game, GameError, RenderState, Scene, Stats,
};
pub use gpu::{Gpu, Paths};
pub use map::MapChoice;
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
