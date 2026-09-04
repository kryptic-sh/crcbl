//! Sundial — the engine's shadow acceptance fixture.
//!
//! One open plaza, a sun on a scripted clock, and every shadow filter the engine
//! ships drawn from the same frame. **Not a game**: the shadow is the content,
//! and there is nothing to play.
//! [`docs/plan/sample/18-sundial.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/18-sundial.md)
//! is the charter, and its non-goals are a hard cap.
//!
//! **No `World`, no system, no `GameModule`**, and their absence is the charter's
//! answer rather than an oversight: sample rules 2 and 10 exist so a *game*'s
//! state lives on the server and its logic in module code, and there is none
//! here. Same ground `apps/lantern`, `apps/viewer` and `apps/alcove` are exempt
//! on, and the charter exempts this sample from rule 11 as well — flat untextured
//! surfaces are the point, because texture detail is exactly what hides a shadow
//! artefact.
//!
//! # What is here at milestone 1
//!
//! The plaza [`plaza`] describes — a large pavement, a colonnade whose shadow
//! crosses a cascade boundary, a plinth resting on the ground so its contact is
//! checkable, three counters hanging at graded heights so a contact-hardening
//! penumbra is visible, and two point lights and a spot, which is exactly what
//! the shadow atlas's light region holds — and every control milestone 1 asks to
//! be *legible*:
//!
//! * **The filter selector and the comparison seam**, which
//!   `docs/plan/45-shadows.md`'s fifteenth decision landed as
//!   `r_shadow_filter` and `r_shadow_split` and which this sample binds to the
//!   `F`, `X`, `,` and `.` keys and to the pause panel's `FILTER` and `SEAM` rows.
//! * **Which filter each side of the seam is running**, on the panel's `NEAR
//!   SIDE` and `FAR SIDE` rows — because two shadowed pictures side by side name
//!   neither.
//! * **The sun on its clock**: [`sun::Clock`] is tick-driven and never reads a
//!   wall clock, so tick `k` is the same sun in every process. `P` stops and
//!   starts it, `-` and `=` scrub it, `R` puts it back, and the panel and the
//!   `[HUD]` line both print where it stands.
//! * **Cost per technique, per frame**: [`gpu::ShadowCost`] reads the atlas
//!   render and the scene draw off `PassTimers`, on the panel and in the headless
//!   summary line. There is no per-side row, and that module's header says why.
//!
//! Beside them: a free-fly camera, the fixed pose the goldens are taken from, a
//! second fixed pose the penumbra ladder is read at, the debug panel rule 4 asks
//! for, the path report and the forcing flags rule 12 asks for, and
//! `tests/golden.rs` — goldens with structural assertions in front of them,
//! because a golden alone cannot make a claim about a shadow and a wrong grey
//! image is a plausible grey image.
//!
//! # What is not here, and where it is written down
//!
//! * **The cascade debug overlay and the atlas viewer.** Both are milestone 1's
//!   and both are *engine* work — a pass that tints by cascade and a pass that
//!   draws the atlas to screen — so neither can be built from an application
//!   crate. `docs/backlog.md` carries them.
//! * **The web demo**, which `docs/backlog.md` carries with what a page needs.
//! * **Milestone 5's ray-traced shadows**, gated on P7C. The panel's `ray
//!   tracing` row says `raster only` rather than implying a choice was made.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here, so
//! `tests/golden.rs` can render the same plaza the binary does. The crate builds
//! for `wasm32-unknown-unknown` as a `cdylib` already, which is what a browser
//! front end will link — there is no `src/web.rs` yet, and the backlog says what
//! one owes.

mod app;
mod args;
pub mod filter;
mod gpu;
pub mod menu;
pub mod plaza;
pub mod sun;

pub use app::{Loop, PendingLoop, Summary, Sundial, SundialError, run, start, with_shell};
pub use args::{
    DEFAULT_TICK_HZ, Invocation, Options, USAGE, binding_from_name, geometry_from_name, parse,
};
/// Re-exported rather than defined here: the free-fly camera lives in
/// `crcbl-render` so every sample flies the same one.
pub use crcbl::render::{Flyer, SPEED, TURN};
pub use filter::Knobs;
pub use gpu::{Forced, Gpu, GpuError, Paths, ShadowCost};
pub use menu::{CameraMode, Menus, SundialAction, action_for, menus, pause_menu, toggled_effect};
