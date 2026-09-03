//! Lantern — the engine's lighting acceptance fixture.
//!
//! One indoor room, chosen for lighting rather than for geometry, rendered under
//! whatever paths the device offers. **Not a game**: the lighting is the
//! content, and there is nothing to play.
//! [`docs/plan/sample/13-lantern.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/13-lantern.md)
//! is the charter, and its non-goals are a hard cap.
//!
//! **No `World`, no system, no `GameModule`**, and their absence is the
//! charter's answer rather than an oversight: sample rules 2 and 10 exist so a
//! *game*'s state lives on the server and its logic in module code, and there is
//! none here — the room is fixed, the lights follow the clock, the camera is the
//! viewer's. Same ground `apps/viewer` is exempt on.
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
//! Ray tracing is out of scope for this milestone and recorded in
//! `docs/backlog.md`. One thing
//! is visible in the picture rather than merely absent from it: **a fully
//! metallic surface has no ambient term, so a reflection is the whole of what
//! lights it**. Both halves of `docs/plan/18-render-features.md`'s probe design
//! are built now — the volume [`bounce`] places is the diffuse half, and a
//! screen-space reflection that finds nothing returns that same volume as its
//! environment — so neither metal surface is black any more. What stands there
//! instead is a probe volume rather than a trace of the room, and above
//! the mirror panel's reflecting band it is the whole of what the face shows:
//! `tests/golden.rs`'s `zero_probes_only_remove_the_ssr_and_rough_fallbacks`
//! zeroes the probe rows and takes that point to nothing while the panel's real
//! screen-space hit stays where it was. Ray tracing is what replaces it, and the
//! debug panel's `ray tracing` row says which path this frame took.
//!
//! **The room bounces the sun**, which it did not before [`bounce`]: that
//! module places the probe volume the scene carries and leaves its rows at zero,
//! and the renderer's reflective shadow map refills them every frame from the
//! sun's first bounce off whatever is standing in the room. It is one bounce and
//! the sun's alone — nothing bounces twice, and a lamp or a torch lends the
//! volume nothing — and that module's docs name each limit. [`room`]'s
//! module docs say the rest, the debug panel's `unbuilt` section says it on
//! screen, and none of it is faked: a fixture whose job is showing what the
//! renderer does must not flatter it.
//!
//! # The monitor, and the layer it is a consumer for
//!
//! **A second camera renders the room into the screen hanging on its back
//! wall.** The charter asks for it as the consumer the per-camera toggle layer
//! did not have: `crcbl_render::effects`' resolution order starts with a stack
//! the *view* asked for, and a frame with one view in it can never show that
//! layer doing anything. [`room::View`] is the two views, [`room::MONITOR_STACK`]
//! is what the monitor's asks for — every effect except the reflections, which
//! `docs/plan/18-render-features.md` names as the thing a render-to-texture
//! camera does not want — and the debug panel's `paths` section prints what each
//! of them resolved to.
//!
//! **The monitor does not reflect itself**, by two mechanisms that answer
//! different questions. [`room::monitor_camera`] stands on the screen's own face
//! and looks out along its normal, so the monitor is behind that camera's near
//! plane; and [`room::place`] is given a [`room::View`], so the renderer that draws the
//! monitor's picture is never handed the screen or its bezel at all. The second
//! is what a change of pose cannot defeat.
//!
//! **The screen is one frame behind**, deliberately: `crate::gpu`'s
//! `feed_monitor` records the second view and the copy that feeds the page at the
//! *tail* of the frame, which is what lets the graph order the copy against the
//! pass that samples it. That module's docs carry the argument.
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
mod gpu;
mod menu;
pub mod room;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Lantern, LanternError, Loop, PendingLoop, Summary, run, start, with_shell};
pub use args::{
    DEFAULT_TICK_HZ, Invocation, Options, USAGE, binding_from_name, geometry_from_name, parse,
};
/// Re-exported rather than defined here: the free-fly camera moved into
/// `crcbl-render` so a second sample could fly the same one, and this keeps
/// `crcbl_lantern::Flyer` resolving for anything that already named it.
pub use crcbl::render::{Flyer, SPEED, TURN};
pub use gpu::{Forced, Gpu, GpuError, Paths, Unbuilt};
pub use menu::{
    AO_ID, CAMERA_ID, CameraMode, LanternAction, Menus, REFLECTIONS_ID, SHADOWS_ID, action_for,
    menus, pause_menu, toggled_effect,
};
