//! The Crucible scene editor: open a `.scn/` scene, pick an entity by ray, edit
//! it through commands, walk the commands back, and save byte-stably.
//!
//! `docs/plan/08-editor.md`'s **smallest slice that uses only what exists**,
//! and the first delivery of the decisions that document records for
//! 2026-09-16.
//!
//! ```text
//!     a click ──▶ Camera::ray_through ──▶ PhysicsSystem::cast_ray ──▶ selection
//!     a key   ──▶ EditCommand ──▶ Document::apply ──▶ UndoLog ──▶ the inverse
//!     Ctrl+S  ──▶ Scene::save ──▶ the same bytes, or exactly the edited ones
//! ```
//!
//! # The shape, and why it is this shape
//!
//! * [`command`] — every edit as a value, and the log. Applied in process here
//!   and routed over the transport later: what a transport gains is a carrier,
//!   not a vocabulary. **No mutation anywhere in this crate is a field write at
//!   the call site.**
//! * [`document`] — the scene, the [`World`](crcbl::ecs::World) it loaded into,
//!   the selection, the history and the save. Runs with no device and no
//!   window, which is how the gates hold it.
//! * [`scene`] — this build's component vocabulary and the document it opens
//!   on. **The only module that names a component type**, and it names no
//!   game: the rest of the crate asks a [`Registry`](crcbl::registry::Registry)
//!   and opens any scene whose systems are in it.
//! * [`app`] — the window, the device and the loop, which does nothing but call
//!   [`document`].
//! * [`args`] — the command line.
//!
//! # What this slice is not
//!
//! No property inspector (the UI rungs of `docs/plan/07-ui-debug.md` are
//! another slice's), no server protocol or transport routing, no gizmos, no
//! play mode, no asset listing, no file watcher, no multi-session editing, and
//! no port of a sample's state into ECS. Each is named in the plan with what it
//! waits on.

pub mod app;
pub mod args;
pub mod command;
pub mod document;
pub mod scene;

pub use app::{Editor, EditorError, Summary, run};
pub use args::{Invocation, Options, USAGE, parse};
pub use command::{EditCommand, UndoLog};
pub use document::{Document, EditError};
