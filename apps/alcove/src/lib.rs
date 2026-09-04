//! Alcove — the engine's ambient-occlusion acceptance fixture.
//!
//! One walled court of nothing but occlusion geometry, rendered under whatever
//! paths the device offers. **Not a game**: the occlusion is the content, and
//! there is nothing to play.
//! [`docs/plan/sample/19-alcove.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/19-alcove.md)
//! is the charter, and its non-goals are a hard cap.
//!
//! **No `World`, no system, no `GameModule`**, and their absence is the
//! charter's answer rather than an oversight: sample rules 2 and 10 exist so a
//! *game*'s state lives on the server and its logic in module code, and there is
//! none here. Same ground `apps/lantern` and `apps/viewer` are exempt on, and
//! the charter exempts this sample from rule 11 more strongly still — flat
//! untextured surfaces are the point, because texture detail is exactly what
//! hides an occlusion artefact.
//!
//! # What is here at milestones 1 and 2
//!
//! The court [`court`] describes — an alcove, a flight of cantilevered treads,
//! boxes resting on a floor, a deep slot the sun runs down, and a sphere against
//! a far wall — and every control the two milestones ask to be *legible*:
//!
//! * **The AO-only view**, which the engine ships as
//!   `ForwardRenderer::set_occlusion_view` and this sample reaches through
//!   `crcbl::debug_view` — the `V` key and the pause panel's `AO VIEW` row.
//! * **Radius and intensity**, live and shown. Both were already console
//!   variables the pass reads every frame; what milestone 1 asked for and a
//!   variable cannot give is that they be legible on screen, which is
//!   [`menu::pause_menu`]'s `RADIUS` and `INTENSITY` rows and the debug panel's
//!   `occlusion` section.
//! * **The technique selector and the comparison seam**, and — this is the whole
//!   of what milestone 2 adds to the engine's half — **which technique each side
//!   of the seam is running**, on the panel's `NEAR SIDE` and `FAR SIDE` rows.
//! * **Cost per technique, per frame**: [`gpu::OcclusionCost`] reads the
//!   occlusion chain's own passes off `PassTimers`, so a frame drawn with the
//!   seam up reports its two gathers separately, on the panel and in the
//!   headless summary line.
//!
//! Beside them: a free-fly camera and a fixed one the goldens are taken from,
//! the debug panel rule 4 asks for, the path report and the forcing flags rule
//! 12 asks for, and `tests/golden.rs` — goldens with structural assertions in
//! front of them, because a golden alone cannot make a claim about occlusion and
//! a wrong grey image is a plausible grey image.
//!
//! # What is not here, and where it is written down
//!
//! * **Milestone 3's bent-normal visualisation.** The switch is reachable —
//!   `r_ssao_bent_normals`, the `B` key and a panel row — but a *visualisation*
//!   of the direction is a debug view of its own, and the charter is explicit
//!   that a term steering where ambient is sampled from cannot be reviewed as a
//!   grey image.
//! * **Milestone 4's ray-traced occlusion**, gated on P7C. The panel's
//!   `ray tracing` row says `raster only` rather than implying a choice was
//!   made.
//! * **The Pages web demo.** `docs/backlog.md`'s "alcove's web demo is owed"
//!   lists the eight places a browser demo has to be registered; this crate is
//!   already a `cdylib` and has no web front end behind it yet.
//!
//! # One library, one front end so far
//!
//! `src/main.rs` is argv and an exit code; everything else is here, so
//! `tests/golden.rs` can render the same court the binary does.

mod app;
mod args;
pub mod court;
mod gpu;
pub mod menu;
pub mod occlusion;

pub use app::{Alcove, AlcoveError, Loop, Summary, run, start, with_shell};
pub use args::{
    DEFAULT_TICK_HZ, Invocation, Options, USAGE, binding_from_name, geometry_from_name, parse,
    seam_from_name, technique_from_name,
};
/// Re-exported rather than defined here: the free-fly camera lives in
/// `crcbl-render` so every sample flies the same one.
pub use crcbl::render::{Flyer, SPEED, TURN};
pub use gpu::{Forced, Gpu, GpuError, OcclusionCost, Paths};
pub use menu::{AlcoveAction, CameraMode, Menus, action_for, menus, pause_menu, toggled_effect};
pub use occlusion::Knobs;
