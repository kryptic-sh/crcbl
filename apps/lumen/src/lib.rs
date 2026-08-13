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
//! Screen-space reflections, irradiance probes, ray tracing, the
//! render-to-texture monitor camera, per-effect toggles and the Pages web demo
//! are all out of scope for this milestone and all recorded in
//! `docs/backlog.md`. Two of them are visible in the picture rather than merely
//! absent from it: **a fully metallic surface has no ambient term and renders
//! near-black**, and the coloured wall does not bounce. [`room`]'s module docs
//! say why, the debug panel's `unbuilt` section says it on screen, and neither
//! is faked — a fixture whose job is showing what the renderer does must not
//! flatter it.
//!
//! # Rule 11 does not apply
//!
//! No `.crpix` art. The charter exempts this sample explicitly: the subject is
//! 3D lighting, and pixel art in front of it would be showing the wrong system.
//!
//! # One library, one binary
//!
//! `src/main.rs` is argv and an exit code; everything else is here, so
//! `tests/golden.rs` can render the same room the binary does. The browser entry
//! point the charter's Scope also asks for is deferred, and the six places a new
//! demo has to be named are listed in `docs/backlog.md`.

mod app;
mod args;
mod camera;
mod gpu;
mod menu;
pub mod room;

pub use app::{Loop, Lumen, LumenError, Summary, run, start, with_shell};
pub use args::{
    DEFAULT_TICK_HZ, Invocation, Options, USAGE, binding_from_name, geometry_from_name, parse,
};
pub use camera::{Flyer, SPEED, TURN};
pub use gpu::{Forced, Gpu, GpuError, Paths, Unbuilt};
pub use menu::{CAMERA_ID, CameraMode, LumenAction, Menus, action_for, menus, pause_menu};
