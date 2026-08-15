//! Lumen — the engine's lighting acceptance fixture.
//!
//! One indoor room, chosen for lighting rather than for geometry, rendered under
//! whatever paths the device offers. **Not a game**: the lighting is the
//! content, and there is nothing to play.
//! [`docs/plan/sample/13-lumen.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/13-lumen.md)
//! is the charter, and its non-goals are a hard cap.
//!
//! # What is here at milestone 1a
//!
//! The room the charter's Scope names — a window the sun comes through, a
//! mirror-grade panel, a rough metal block, a coloured wall and a moving light —
//! described by **this crate** rather than by the engine, which is the whole of
//! what `crcbl::render::scene` bought and what this sample is the first consumer
//! of. Beside it: a free-fly camera and a fixed one the goldens are taken from,
//! the debug panel rule 4 asks for, the path report and the forcing flags rule
//! 12 asks for, and `tests/golden.rs` — a golden frame with structural
//! assertions in front of it, because a golden alone cannot make a claim about
//! lighting and a wrong picture is a plausible picture.
//!
//! # What is not here, and where it is written down
//!
//! Ray tracing and the render-to-texture monitor camera are out of scope for
//! this milestone and recorded in `docs/backlog.md`. One thing
//! is visible in the picture rather than merely absent from it: **a fully
//! metallic surface has no ambient term, so a reflection is the whole of what
//! lights it**. Both halves of `docs/plan/18-render-features.md`'s probe design
//! are built now — the volume [`bounce`] bakes is the diffuse half, and a
//! screen-space reflection that finds nothing returns that same volume as its
//! environment — so neither metal surface is black any more. What stands there
//! instead is a *baked* environment rather than a trace of the room, and above
//! the mirror panel's reflecting band it is the whole of what the face shows:
//! `tests/golden.rs`'s `zero_probes_only_remove_the_ssr_and_rough_fallbacks`
//! zeroes the probe rows and takes that point to nothing while the panel's real
//! screen-space hit stays where it was. Ray tracing is what replaces it, and the
//! debug panel's `ray tracing` row says which path this frame took.
//!
//! **The coloured wall does bounce**, which it did not before [`bounce`]: a
//! single analytic gather of the sun's first bounce off the room's interior,
//! computed from the room's own dimensions into the probe volume the scene
//! carries. It is one bounce against one axis-aligned box and not a general
//! global-illumination solution — nothing inside the room occludes it and
//! nothing bounces twice — and that module's docs name each limit. [`room`]'s
//! module docs say the rest, the debug panel's `unbuilt` section says it on
//! screen, and none of it is faked: a fixture whose job is showing what the
//! renderer does must not flatter it.
//!
//! # Rule 11 does not apply
//!
//! No `.crpix` art. The charter exempts this sample explicitly: the subject is
//! 3D lighting, and pixel art in front of it would be showing the wrong system.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here, so
//! `tests/golden.rs` can render the same room the binary does. `src/web.rs` is
//! the second front end — compiled only on `wasm32`, which is why it is not
//! linked on a host build — and what it publishes is the charter's reason for
//! wanting it: a browser has no ray query, so the page draws the room through
//! [`crcbl::hal::LightingPath::Rasterised`] by construction and is the one place
//! that path can be looked at without building anything.

mod app;
mod args;
pub mod bounce;
mod camera;
mod gpu;
mod menu;
pub mod room;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, Lumen, LumenError, PendingLoop, Summary, run, start, with_shell};
pub use args::{
    DEFAULT_TICK_HZ, Invocation, Options, USAGE, binding_from_name, geometry_from_name, parse,
};
pub use camera::{Flyer, SPEED, TURN};
pub use gpu::{Forced, Gpu, GpuError, Paths, Unbuilt};
pub use menu::{
    AO_ID, CAMERA_ID, CameraMode, LumenAction, Menus, REFLECTIONS_ID, SHADOWS_ID, action_for,
    menus, pause_menu, toggled_effect,
};
