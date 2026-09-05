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
//! # What is here
//!
//! The plaza [`plaza`] describes — a large pavement, a colonnade whose shadow
//! crosses a cascade boundary, a plinth resting on the ground so its contact is
//! checkable, three counters hanging at graded heights so a contact-hardening
//! penumbra is visible, and two point lights and a spot, which is exactly what
//! the shadow atlas's light region holds — and every control the charter's
//! first four milestones ask to be *legible*:
//!
//! * **The filter selector and the comparison seam**, which
//!   `docs/plan/45-shadows.md`'s fifteenth decision landed as
//!   `r_shadow_filter` and `r_shadow_split` and which this sample binds to the
//!   `F`, `X`, `,` and `.` keys and to the pause panel's `FILTER` and `SEAM` rows.
//! * **Which filter each side of the seam is running**, on the panel's `NEAR
//!   SIDE` and `FAR SIDE` rows — because two shadowed pictures side by side name
//!   neither.
//! * **The sun's two bias counts**, which the same plan's seventh decision
//!   landed and which are `r_shadow_bias` and `r_shadow_normal_offset` here:
//!   `[` and `]` walk one, `;` and `'` walk the other, `9`/`0` and `7`/`8` walk
//!   the same two in coarse steps that reach the far end of either range, and
//!   the panel's `BIAS` and `NORMAL OFFSET` rows say where they stand. Milestone
//!   2 is the pair of artefacts they trade against each other — acne where a
//!   count is too small, a shadow off its caster where one is too large — and
//!   neither is a thing a still frame shows.
//! * **The sun on its clock**: [`sun::Clock`] is tick-driven and never reads a
//!   wall clock, so tick `k` is the same sun in every process. `P` stops and
//!   starts it, `-` and `=` scrub it, `R` puts it back, and the panel and the
//!   `[HUD]` line both print where it stands.
//! * **Cost per technique, per frame**: [`gpu::ShadowCost`] reads the atlas
//!   render and the scene draw off `PassTimers`, on the panel and in the headless
//!   summary line. There is no per-side row, and that module's header says why.
//! * **Milestone 1's two diagnostics.** `C` and the panel's `CASCADES` row draw
//!   [`crcbl::render::DebugView::Cascades`], which colours the frame by the
//!   cascade each sun-lit fragment read; `T` and the `ATLAS` row draw
//!   [`crcbl::render::DebugView::ShadowAtlas`], which is the atlas itself — so
//!   which of the plaza's three punctual lights was given a tile is something to
//!   look at rather than to infer from a scene that lights either way.
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
//! * **Milestone 5's ray-traced shadows**, gated on P7C. The panel's `ray
//!   tracing` row says `raster only` rather than implying a choice was made.
//!
//! # One library, three front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here, so
//! `tests/golden.rs` can render the same plaza the binary does. `src/web.rs` is
//! the browser's — compiled only on `wasm32`, which is why it is not linked on a
//! host build — and it carries what a page needs on top of the shared boot
//! protocol: the filter, the seam and the sun's clock as exports of their own,
//! because natively they are keys and a phone has none.

mod app;
mod args;
pub mod filter;
mod gpu;
pub mod menu;
pub mod plaza;
pub mod sun;

#[cfg(target_arch = "wasm32")]
pub mod web;

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
