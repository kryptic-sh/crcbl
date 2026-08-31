//! The lit mesh through the render graph — its goldens, its HDR scene target,
//! its resize storm and its uniform level-of-detail cut — on whichever backend
//! the registry opens.
//!
//! ```text
//! CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh [extra nextest args…]
//! ```
//!
//! # Why this exists
//!
//! `crates/crcbl-vk/tests/vk_e2e/mesh.rs` was the last of the five clusters that
//! ran on Vulkan alone, and the only one whose migration was a redesign rather
//! than a move. Its fifteen tests split three ways:
//!
//! * **Nothing about the frame is Vulkan's**, so the four goldens, the
//!   directional-light gradient, the `Rgba16Float` peak and the transient pool's
//!   behaviour under resize are here, unchanged in substance. Every one of them
//!   used to be evidence about radv and lavapipe and about nothing else.
//! * **The mesh-shader path is Vulkan's**, and the tests that name it stayed:
//!   wgpu, WARP and Metal have no mesh stage at all, so an assertion that
//!   [`GeometryPath::MeshShader`](crcbl::hal::GeometryPath) was selected is a
//!   claim about a driver rather than about the seam. `vk_e2e/mesh.rs`'s header
//!   lists what is left and why each one could not come.
//! * **And two were split in half.** The multi-cluster open box drew one golden
//!   *and* compared two geometry paths byte for byte: the picture is the seam's
//!   and is [`goldens`]'s, the comparison is the driver's and stayed. The dunes
//!   patch's level selection asserted a per-cluster cut against a uniform one:
//!   the uniform cut is what every backend runs and is [`lod`]'s, the
//!   per-cluster comparison needs an amplification stage and stayed.
//!
//! Splitting rather than gating is the point. Moving those tests whole and
//! wrapping their assertions in a mesh-shader capability check would leave three
//! backends running a test whose substance is skipped, which reports "not
//! supported here" as "passed" — the shape `docs/plan/12-testing.md` calls a
//! known trap and this repo keeps removing.
//!
//! # The goldens moved with the tests
//!
//! `tests/golden/mesh.png`, `mesh_ortho.png`, `mesh_second.png` and
//! `mesh_clusters.png` were `crates/crcbl-vk/tests/golden/`'s. They are compared
//! at [`Tolerance::RASTERISER`](crcbl_golden::Tolerance::RASTERISER), the same
//! bound `tests/render_e2e.rs` and `tests/sprite_e2e/` hold every backend to.
//! They were blessed on Vulkan and are **not** re-blessed per backend: a backend
//! that cannot meet them is a finding.
//!
//! `mesh_clusters.png` is the loosest reference in the tree and is worth knowing
//! about before a failure here is read as a regression — its measured headroom
//! is recorded in `docs/backlog.md`.
//!
//! `vk_e2e/mesh.rs` still checks `mesh.png` and `mesh_clusters.png` from its own
//! side, through this directory rather than through a second copy: one reference
//! file is the whole reason those tests are evidence that two paths draw the
//! *same* picture.
//!
//! # The backend must be named
//!
//! Every backend is meant to draw these frames identically, so a run that
//! silently fell back to another backend passes and proves nothing about the one
//! that was wanted. [`harness`]'s `instance` compares the opened backend against
//! `CRCBL_GPU`, and `tests/run-mesh-e2e.sh` refuses to run without it. Same
//! contract, same reason and the same runner conventions as
//! `run-draw-gen-e2e.sh`.

#![cfg(feature = "mesh-e2e")]

/// What this binary calls itself in every line it prints and every debug label
/// it sets.
///
/// Read by `tests/gpu_scene/harness.rs`, which is shared with three other
/// suites and therefore cannot name any of them. **This string is
/// `tests/run-mesh-e2e.sh`'s grep**: the runner finds the adapter line by its
/// prefix, so changing it turns a green suite into a failed harness run.
pub(crate) const SUITE: &str = "crcbl mesh e2e";

// The suite's areas, one module each, all in `tests/mesh_e2e/`. The root is
// `tests/mesh_e2e/main.rs`, so Cargo compiles the directory as one test binary
// named `mesh_e2e` and every `mod` here resolves beside the root.
mod area_light;
mod debug_draw;
mod depth_only;
mod exposure;
mod froxels;
mod goldens;
mod hdr;
mod lod;
mod motion;
mod normal_map;
mod render_scale;
mod resize;
mod shadow_tiles;
mod skinned_motion;
mod smaa;
mod two_dags;
mod vertex_v2;

// The fixture, out of `tests/gpu_scene/` rather than beside the root, because
// three other suites open the same device against the same offscreen ring and a
// second copy is a second place a fix has to land. That directory holds no
// `main.rs`, so Cargo builds no target of its own from it.
#[path = "../gpu_scene/harness.rs"]
mod harness;

// The scene, in a second file for the reason that one's header gives. This
// suite is the third of the three that draw it, and the only one that asks
// `render_mesh` for the `Rgba16Float` target beside the swapchain image.
#[path = "../gpu_scene/mesh_scene.rs"]
mod mesh_scene;
