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
//! This is that document's **milestone 1**, "Load + orbit + grid", and the
//! whole of milestone 2. What it does: takes a path, reads it through the asset
//! seam, converts it, frames the camera on it, turns it under the mouse, draws
//! it under a single directional light over a grid floor, and puts what the
//! document holds — and what the conversion could not bring in — on screen
//! behind `I`. See [`listing`]. `W` draws it in wireframe and `N` in
//! world-space normals; `-` and `=` step the exposure, and so does the slider
//! on the `ESC` panel — see [`menu`]. Re-export the file and the frame becomes
//! the new document, which is milestone 3's artist loop — see [`watch`].
//!
//! # It runs in a browser too, on a document it brings with it
//!
//! `crate::web` is the `wasm32` front end — not linked, because it does not
//! exist in a native documentation build — and `web/demos/viewer/` is its
//! page. A tab
//! has no path to type and no directory to root an asset source at, so it opens
//! the document [`demo_model`] generates and compiles into the module —
//! everything past the loading is the code below, unchanged.
//!
//! What it still deliberately does not do: **no drop target**. V-F5 is "path
//! argument natively, drop target in the browser", and the second half is a
//! document the *visitor* chooses. That needs an asset source over bytes the
//! browser handed across, which is now a small piece of work rather than the
//! stage-10 item it once was — [`crcbl::assets::MemorySource`] is the part that
//! was missing, and the browser demo already loads through one. It is in
//! `docs/backlog.md` with what is left.
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
pub mod demo_model;
pub mod gpu;
pub mod listing;
pub mod menu;
pub mod model;
pub mod watch;

#[cfg(target_arch = "wasm32")]
pub mod web;

/// The `.glb` documents this crate's own tests open.
///
/// Not compiled into the binary: it is a fixture, and `crcbl-scene`'s
/// equivalent is `pub(crate)` there for the same reason.
#[cfg(test)]
mod fixture;

pub use app::{
    LISTING_KEY, Loop, PendingLoop, REFRAME_KEY, Summary, Viewer, ViewerError, run, start,
    with_shell,
};
pub use args::{DEFAULT_TICK_HZ, Invocation, Options, USAGE, parse};
pub use controls::Controls;
pub use listing::Listing;
pub use menu::{MenuKind, Menus};
pub use model::{LoadError, Model, Rig, load, load_from, world_bounds};
