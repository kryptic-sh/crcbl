//! viewer — open a glTF file, frame it, turn it, look at it.
//!
//! ```text
//! viewer <MODEL> [OPTIONS]
//! ```
//!
//! Not a game: a tool, and the asset pipeline's acceptance test. Every other
//! sample draws content this workspace authored and knows to be good;
//! `docs/plan/sample/05-viewer.md` exists because that proves nothing about a
//! `.glb` out of Blender, Sketchfab or the Khronos sample suite, and the way to
//! find out is to point something at one and see.
//!
//! # What is here, and what is not
//!
//! This is that document's **milestone 1**, "Load + orbit + grid", minus the
//! grid. What it does: takes a path, reads it through the asset seam, converts
//! it, frames the camera on it, turns it under the mouse, and draws it under a
//! single directional light.
//!
//! What it deliberately does not do, each because it needs something this slice
//! does not build:
//!
//! * **No grid floor.** A grid is a second resident mesh and a second material
//!   row in a [`SceneDesc`](crcbl::render::scene::SceneDesc) that
//!   [`build_render_scene`](crcbl::scene::gltf_render::build_render_scene) sized
//!   for the document alone, so it is a change to how the scene is assembled and
//!   not a pass to add — see [`gpu`].
//! * **No mesh, material or texture panels.** Milestone 2's listings are the
//!   viewer's own UI, and there is none: [`gpu`] has a UI pass and everything
//!   that goes through it — the menu and rule 4's debug panel — is the engine's.
//! * **No exposure slider**, which is the same UI and a renderer control this
//!   milestone does not reach for.
//! * **No hot reload.** Milestone 3, and the whole of the artist loop the
//!   sample doc's V-F4 describes.
//! * **No drop target.** V-F5 is "path argument natively, drop target in the
//!   browser", and this is the native half. The browser half needs an asset
//!   source over a dropped file, which stage 10 owns.
//!
//! Every one of those is in `docs/backlog.md` with what it is waiting on.
//!
//! # Two rules this sample is exempt from, and neither is an oversight
//!
//! **Rule 2, client/server authority.** `docs/plan/sample/05-viewer.md` names
//! this sample as the one sanctioned exception: the rule exists so that a
//! *game*'s state is the server's, and this simulates nothing at all. There is
//! no tick in [`app`] and no [`GameModule`](crcbl::ecs::GameModule) anywhere.
//!
//! **Rule 11, `.crpix` art through the sprite pass.** Also named in that
//! document. The whole point of a viewer is that it shows *the user's* asset
//! unadorned; authored art in the viewport would be exactly the thing it must
//! not do. There is no `build.rs` here and no bake step.
//!
//! # A file that will not load is a message and an exit code
//!
//! This is the one application in the tree a non-developer is meant to point at
//! an arbitrary file, so [`model`] turns every way a document can be wrong into
//! a sentence naming the file and what to do — and everything the conversion
//! could not honour is printed where the person who opened the file is looking,
//! not only logged. See [`model`] and [`app::run`].

pub mod app;
pub mod args;
pub mod controls;
pub mod gpu;
pub mod menu;
pub mod model;

/// The `.glb` documents this crate's own tests open.
///
/// Not compiled into the binary: it is a fixture, and `crcbl-scene`'s
/// equivalent is `pub(crate)` there for the same reason.
#[cfg(test)]
mod fixture;

pub use app::{Loop, Summary, Viewer, ViewerError, run, start, with_shell};
pub use args::{DEFAULT_TICK_HZ, Invocation, Options, USAGE, parse};
pub use controls::Controls;
pub use menu::{MenuKind, Menus};
pub use model::{LoadError, Model, load, world_bounds};
