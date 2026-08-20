//! Quarry — the geometry acceptance fixture.
//!
//! `docs/plan/sample/14-quarry.md`, phase S4C. One dense scene drawn on every
//! [`GeometryPath`](crcbl::hal::GeometryPath), with the cluster hierarchy made
//! visible. Not a game: the geometry is the content.
//!
//! Where `apps/lumen` proves the two lighting paths agree, this proves the
//! three geometry paths do — and it is the only place the QEM generator's
//! output is *looked at* rather than measured, because an error metric can be
//! inside its budget while the mesh is visibly wrong at a seam.
//!
//! # What is here so far
//!
//! Milestones 1 to 4, less the two overlays milestone 2 owes. [`face`]
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
//! Still owed, from that document's milestones: the freeze-selection camera, of
//! milestone 2. Its two siblings are built — `--lod-view` tints each cluster by
//! the DAG level it was decimated to, `--heatmap` shades it by the projected
//! error the selection judged it on — and both are mesh-path only, because a
//! per-cluster number exists only where selection is per cluster. The skinned
//! prop is behind an engine feature that does not exist — nothing here does
//! skinning.
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
    CAMERA_ID, CameraMode, HEATMAP_ID, LOD_VIEW_ID, Menus, QuarryAction, action_for, menus,
    pause_menu, toggled_to,
};
