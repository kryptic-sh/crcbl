//! The draw-generation cluster — cull, its statistics ring, and the indirect
//! arguments it feeds — on whichever backend the registry opens.
//!
//! ```text
//! CRCBL_GPU=vk crates/crcbl/tests/run-draw-gen-e2e.sh [extra nextest args…]
//! ```
//!
//! # Why this exists
//!
//! These tests **read GPU-written buffers back and compare them against a CPU
//! oracle**, which is a check no golden image can make: a wrong `first_index`
//! that names the same range and a right one produce the same pixels, and a cull
//! that rejects nothing draws the same frame as one that works. They lived in
//! `crates/crcbl-vk/tests/vk_e2e/` and therefore ran on Vulkan alone, so Metal,
//! D3D12 and wgpu got `render_e2e`'s black-box goldens and nothing else — a bug
//! in the cull pass or the draw-args buffers on those backends surfaced as "some
//! scene looks wrong" rather than as "this buffer holds the wrong value".
//!
//! The gap grew when the draw-args pass was repacked from fourteen storage
//! buffers to eight: five host-static tables merged into one word buffer with
//! offsets in `Params`, the survivor list sharing a buffer with the per-bucket
//! runs via `bucket_base`, and the counts sharing one with the mesh-dispatch
//! extents. Every one of those is an offset a backend can get wrong without
//! changing a pixel.
//!
//! Nothing here names a backend type — only `crcbl::hal`, `crcbl::render`,
//! `crcbl::shaders` and `crcbl::backend::open` — so one binary run four times is
//! the whole matrix, exactly as `tests/hal_seam_e2e.rs` and `tests/render_e2e.rs`
//! are.
//!
//! # The backend must be named
//!
//! Every backend is meant to compute these buffers identically, so a run that
//! silently fell back to another backend passes and proves nothing about the one
//! that was wanted. [`harness`]'s `instance` compares the opened backend against
//! `CRCBL_GPU`, and `tests/run-draw-gen-e2e.sh` refuses to run without it. Same
//! contract, same reason, same runner conventions as `run-hal-seam-e2e.sh`.
//!
//! # What stayed behind in `vk_e2e`
//!
//! `vk_e2e/draw_gen.rs` keeps `every_geometry_path_draws_the_same_frame`. Its
//! three arms assert a *named* [`GeometryPath`](crcbl::hal::GeometryPath) each,
//! reached by subtracting features from an adapter that reports all of them —
//! which is a claim about radv and lavapipe rather than about the seam. Metal
//! has no GPU-side draw count at all, so its `IndirectCount` arm cannot exist,
//! and a backend without a mesh stage cannot reach `MeshShader`. The
//! cross-backend form of that claim is already owned by `render_e2e.rs`'s
//! `the_*_scene_draws_the_same_frame_on_every_geometry_path`, which derives the
//! expected paths from what the adapter reports instead of writing them down.

#![cfg(feature = "draw-gen-e2e")]

/// What this binary calls itself in every line it prints and every debug label
/// it sets.
///
/// Read by `tests/gpu_scene/harness.rs`, which is shared with
/// `tests/forward_e2e/` and therefore cannot name either of them. **This string
/// is `tests/run-draw-gen-e2e.sh`'s grep**: the runner finds the adapter line by
/// its prefix, so changing it turns a green suite into a failed harness run.
pub(crate) const SUITE: &str = "crcbl draw gen e2e";

// The suite's areas, one module each, all in `tests/draw_gen_e2e/`. The root is
// `tests/draw_gen_e2e/main.rs`, so Cargo compiles the directory as one test
// binary named `draw_gen_e2e` and every `mod` here resolves beside the root.
mod cull;
mod cull_stats;
mod draw_gen;

// The fixture, out of `tests/gpu_scene/` rather than beside the root, because
// `tests/forward_e2e/` and `tests/sprite_e2e/` open the same device against the
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

// The culling-statistics readback, in a third file for the reason that one's
// header gives: `tests/mesh_e2e/` renders the same scene and reads no counter,
// so a scene and a statistics copy in one file left this dead code there.
#[path = "../gpu_scene/cull_readback.rs"]
mod cull_readback;
