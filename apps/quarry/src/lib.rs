//! Quarry — the geometry acceptance fixture.
//!
//! `docs/plan/sample/14-quarry.md`, phase S4C. One dense scene drawn on every
//! [`GeometryPath`](crcbl::hal::GeometryPath), with the cluster hierarchy made
//! visible. Not a game: the geometry is the content.
//!
//! **No `World`, no system, no `GameModule`**, and their absence is the
//! charter's answer rather than an oversight: sample rules 2 and 10 exist so a
//! *game*'s state lives on the server and its logic in module code, and there
//! is no game state here — one face, one instance, a camera and a debug view
//! selector. `docs/plan/sample/14-quarry.md`'s non-goals say so, on the ground
//! `apps/viewer` is exempt on.
//!
//! Where `apps/lantern` proves the two lighting paths agree, this proves the
//! three geometry paths do — and it is the only place the QEM generator's
//! output is *looked at* rather than measured, because an error metric can be
//! inside its budget while the mesh is visibly wrong at a seam.
//!
//! # What is here so far
//!
//! Milestones 1 to 4, whole. [`face`]
//! generates the content, [`scene`] describes it as one flat mesh and [`dag`]
//! as a cluster hierarchy,
//! and `tests/device/residency.rs` draws both through the real renderer on an
//! offscreen context — so the `MeshShader` path rendering the scene is asserted
//! rather than looked at.
//!
//! Per-cluster selection over that hierarchy is asserted too: the face draws
//! from more than one level at once, and no level dominates the cut.
//!
//! The fixed dolly runs too, on one renderer so that hysteresis is in play, and
//! detail measurably arrives as the camera closes without the cut jumping.
//!
//! All three [`GeometryPath`](crcbl::hal::GeometryPath) values draw it, forced
//! by subtracting features from one adapter, and they agree about the frame.
//! Six golden frames — three paths at each end of the dolly — are committed
//! under `tests/golden/`.
//!
//! One assertion is not a count — that the face is shaded by the light it is
//! given — because every other one would survive a face lit from the wrong side.
//!
//! [`tile`] is milestone 4's modular wall piece: two tiles decimated apart still
//! meet, bit for bit.
//!
//! **And there is a window.** [`run`] opens one over the cluster DAG, with the
//! three cameras the pause menu cycles — the goldens' dolly held still, that
//! same dolly run down the face and back on the simulation clock, and a free-fly
//! camera sized for a face 180 metres deep — the LOD tint overlay,
//! `--lod-budget`, the path forcing rule 12 asks for and the debug panel rule 4
//! asks for. A run with `--headless --frames N` prints which paths its frames
//! took, where the camera is, the triangle count and how the cut split between
//! instance and cluster culling, which is what
//! `docs/plan/sample/14-quarry.md`'s exit criteria ask be recorded.
//!
//! **And there is a browser page.** `src/web.rs` is the second front end, compiled
//! only on `wasm32`; it opens on the animated dolly, because a page showing one
//! held frame proves nothing about a cut that follows the camera. WebGPU exposes
//! neither a mesh stage nor a GPU-side draw count, so it is also the one place
//! [`crcbl::hal::GeometryPath::IndirectPerBatch`] can be looked at without
//! forcing it.
//!
//! Milestone 2's three debug overlays are all built. `--lod-view` tints each
//! cluster by the DAG level it was decimated to and `--heatmap` shades it by the
//! projected error the selection judged it on — both mesh-path only, because a
//! per-cluster number exists only where selection is per cluster. [`FREEZE_KEY`]
//! is the third: it pins the eye the cut is chosen from, so a reviewer can fly
//! away from that viewpoint and look at the boundaries the cut drew. It is not
//! an overlay and composes with both of the others, which is the combination it
//! is for.
//!
//! Still owed, from that document's milestones: the skinned prop. The blocker
//! is not that the engine cannot skin — `crcbl_render::skinning`, `crcbl-anim`
//! and `apps/puppet` all ship, and a golden holds the pose a palette asks for.
//! It is that skin weights do not survive a decimation collapse, which
//! `crcbl_scene::simplify`'s own header states: the quadric is over positions,
//! and a collapse has no rule for the weights of the vertex it removes.
//!
//! # Rule 11 does not apply
//!
//! No `.crpix` art. The charter exempts this sample explicitly: the subject is
//! 3D geometry density, and pixel art in front of it would be showing the wrong
//! system.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv, an exit code and the device-free `--report`;
//! everything else is here, so `tests/device/` renders the same face the binary
//! does and flies the same dolly. `src/web.rs` is the second front end — the
//! `extern "C"` entry point a browser's JS shim drives once per
//! `requestAnimationFrame`, compiled only on `wasm32`, which is why a host build
//! does not link it.

mod app;
mod args;
pub mod camera;
pub mod dag;
pub mod face;
mod gpu;
mod menu;
pub mod scene;
pub mod tile;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{
    Loop, PendingLoop, Quarry, QuarryError, Summary, cull_row, dolly_at, run, start, with_shell,
};
pub use args::{
    DEFAULT_TICK_HZ, Invocation, Options, USAGE, binding_from_name, geometry_from_name, parse,
};
pub use gpu::{CELLS, Forced, Gpu, GpuError, Paths};
pub use menu::{
    CAMERA_ID, CameraMode, FREEZE_ID, FREEZE_KEY, HEATMAP_ID, LOD_VIEW_ID, Menus, QuarryAction,
    action_for, menus, pause_menu,
};
