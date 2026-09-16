//! The Crucible scene editor: open a `.scn/` scene, pick an entity by ray, edit
//! it through commands, walk the commands back, and save byte-stably.
//!
//! `docs/plan/08-editor.md`'s **smallest slice that uses only what exists**,
//! and the first delivery of the decisions that document records for
//! 2026-09-16.
//!
//! ```text
//!     a click ──▶ Camera::ray_through ──▶ PhysicsSystem::cast_ray ──▶ selection
//!     a row   ──▶ OutlinerState ──▶ the same selection, both ways
//!     a field ──▶ FieldEdit ──▶ EditCommand ──▶ Document::apply ──▶ the inverse
//!     a key   ──▶ ActionMap ──▶ EditCommand ──▶ Document::apply ──▶ UndoLog
//!     Ctrl+S  ──▶ Scene::save ──▶ the same bytes, or exactly the edited ones
//! ```
//!
//! # The shape, and why it is this shape
//!
//! * [`command`] — every edit as a value, and the log. Applied in process here
//!   and routed over the transport later: what a transport gains is a carrier,
//!   not a vocabulary. **The only field write in this crate is the one
//!   [`Document::record_edit`] rewinds a panel's own edit with**, so that the
//!   command it then applies records an exact inverse; every other mutation is
//!   a command.
//! * [`document`] — the scene, the [`World`](crcbl::ecs::World) it loaded into,
//!   the selection, the history and the save. Runs with no device and no
//!   window, which is how the gates hold it.
//! * [`scene`] — this build's component vocabulary and the document it opens
//!   on. **The only module that names a component type**, and it names no
//!   game: the rest of the crate asks a [`Registry`](crcbl::registry::Registry)
//!   and opens any scene whose systems are in it.
//! * [`panel`] — the docked outliner and inspector, and the two-way join
//!   between what they show and what the document holds. Runs with no device
//!   either: a frame of it is a `crcbl_ui` tree laid out and emitted, and its
//!   own tests drive real clicks at real rectangles with no GPU.
//! * [`layout`] — where the panels are, and the settings key that outlives the
//!   run.
//! * [`keys`] — the keyboard, as an `ActionMap` whose reserved `ui` and `text`
//!   contexts decide whether a key is the editor's or a panel's.
//! * [`app`] — the window, the device and the loop, which does nothing but call
//!   [`document`] and [`panel`].
//! * [`args`] — the command line.
//!
//! # What this slice is not
//!
//! No server protocol or transport routing, no gizmos, no play mode, no asset
//! listing, no file watcher, no multi-session editing, and no port of a
//! sample's state into ECS. Each is named in the plan with what it waits on.
//!
//! And the viewport is a **hole the panels leave in a full-window render**
//! rather than a view of its own: nothing in the tree can draw a GPU texture
//! and nothing in the render graph can scissor a pass to a sub-rectangle, so
//! `docs/plan/08-editor.md`'s open question about the viewport is still open.
//! [`panel`]'s docs say what closing it would take.

pub mod app;
pub mod args;
pub mod command;
pub mod document;
pub mod keys;
pub mod layout;
pub mod panel;
pub mod scene;

pub use app::{Editor, EditorError, Summary, run};
pub use args::{Invocation, Options, USAGE, parse};
pub use command::{EditCommand, UndoLog};
pub use document::{Document, EditError};
pub use panel::{PanelFrame, PanelInput, Panels};
