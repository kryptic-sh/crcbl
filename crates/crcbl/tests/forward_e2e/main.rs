//! The forward pass's own state — its light grid, its shadow atlas, its depth
//! convention and its second colour attachment — on whichever backend the
//! registry opens.
//!
//! ```text
//! CRCBL_GPU=vk crates/crcbl/tests/run-forward-e2e.sh [extra nextest args…]
//! ```
//!
//! # Why this exists
//!
//! Every check here reads something **a rendered picture cannot show**, which is
//! what puts them outside `tests/render_e2e.rs`'s goldens:
//!
//! * [`lights`] reads the froxel grid and the clustering pass's overflow
//!   counter. A grid that silently dropped every light past its budget draws a
//!   frame a little dark in the busy places and matches any golden blessed from
//!   it.
//! * [`shadow`] reads the shadow atlas as depth. A shadow pass that rendered
//!   nothing, or rendered into a map nothing samples, produces a frame in which
//!   every surface is lit and no artifact appears anywhere.
//! * [`depth_probe`] reads the reflectivity attachment, and renders the same
//!   geometry through two projections that differ in nothing else. Reversed-Z is
//!   a decision a comment can claim and only a discriminating pair can check,
//!   and nothing in a picture shows what the second colour target holds.
//!
//! They lived in `crates/crcbl-vk/tests/vk_e2e/` and therefore ran on Vulkan
//! alone, so Metal, D3D12 and wgpu had `render_e2e`'s black-box goldens and
//! nothing else — a shadow atlas that was never written on one of those
//! backends surfaced as a scene that looked fine.
//!
//! Nothing here names a backend type — only `crcbl::hal`, `crcbl::render`,
//! `crcbl::shaders` and `crcbl::backend::open` — so one binary run four times is
//! the whole matrix, exactly as `tests/hal_seam_e2e.rs`, `tests/render_e2e.rs`
//! and `tests/draw_gen_e2e/` are.
//!
//! # A suite of its own rather than more of `draw_gen_e2e`
//!
//! The two share [`harness`] and nothing else. `draw_gen_e2e` asks what the cull
//! and draw-argument passes *computed* — buffers checked against a CPU oracle,
//! with no lighting in the question at all — and this asks what the forward pass
//! *shaded and wrote*. They are separate CI steps for the reason `hal-seam-e2e`
//! and `render-e2e` are: a machine that can run one should not have to run the
//! other to get an answer.
//!
//! # The backend must be named
//!
//! Every backend is meant to produce these buffers and these pixels identically,
//! so a run that silently fell back to another backend passes and proves nothing
//! about the one that was wanted. [`harness`]'s `instance` compares the opened
//! backend against `CRCBL_GPU`, and `tests/run-forward-e2e.sh` refuses to run
//! without it.
//!
//! # What stayed behind in `vk_e2e`
//!
//! `vk_e2e/mesh.rs` and the sprite cluster, because they own golden PNGs blessed
//! on Vulkan. Moving those is a re-blessing decision rather than a move, and
//! `docs/backlog.md` is where it belongs.

#![cfg(feature = "forward-e2e")]

/// What this binary calls itself in every line it prints and every debug label
/// it sets.
///
/// Read by `tests/gpu_scene/harness.rs`, which is shared with
/// `tests/draw_gen_e2e/` and therefore cannot name either of them. **This string
/// is `tests/run-forward-e2e.sh`'s grep**: the runner finds the adapter line by
/// its prefix, so changing it turns a green suite into a failed harness run.
pub(crate) const SUITE: &str = "crcbl forward e2e";

// The suite's areas, one module each, all in `tests/forward_e2e/`. The root is
// `tests/forward_e2e/main.rs`, so Cargo compiles the directory as one test
// binary named `forward_e2e` and every `mod` here resolves beside the root.
mod depth_probe;
mod lights;
mod shadow;

// The fixture, out of `tests/gpu_scene/` rather than beside the root, because
// `tests/draw_gen_e2e/` and `tests/sprite_e2e/` open the same device against the
// same offscreen ring and a second copy is a second place a fix has to land.
// That directory holds no `main.rs`, so Cargo builds no target of its own from
// it.
#[path = "../gpu_scene/harness.rs"]
mod harness;

// The scene, in a second file for the reason that one's header gives: the sprite
// suite opens the fixture and renders no mesh, so a fixture and a scene in one
// file made every symbol here dead code in that binary. Only the two suites that
// draw meshes name it.
#[path = "../gpu_scene/mesh_scene.rs"]
mod mesh_scene;
