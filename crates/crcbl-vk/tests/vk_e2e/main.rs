//! End-to-end suite against a **real Vulkan implementation**.
//!
//! ```text
//! crates/crcbl-vk/tests/run-vk-e2e.sh [extra nextest args…]
//! ```
//!
//! Feature-gated *and* `#[ignore]`d, exactly like `crcbl-shell`'s two
//! window-system suites: `cargo nextest run --workspace --all-features` on a
//! machine with no Vulkan loader must stay green, and the harness script is the
//! only thing that turns these on — and it fails when the suite reports zero
//! tests run, because `docs/plan/12-testing.md` calls a silently-skipped e2e job
//! a known trap.
//!
//! Everything here runs **headless**, through
//! [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen). That is
//! deliberate: it is the only Vulkan CI can run without a compositor, it is the
//! path `crcbl screenshot` and the P1 golden-image e2e need, and it goes through
//! the *same* acquire/present code as a window rather than a second,
//! less-exercised one. The windowed paths are covered by the sandbox runs in
//! `run-wayland-e2e.sh` and `run-x11-e2e.sh`.
//!
//! # Every test asserts a clean validation report
//!
//! [`ValidationReport::assert_clean`] fails on any error *or* warning, and also
//! fails when the layer was never loaded — so a green run means the layer looked
//! and found nothing, not that nobody looked. That is what makes
//! `docs/plan/02-vulkan-backend.md`'s "zero validation errors/warnings" exit
//! criterion a test result.

#![cfg(feature = "vk-e2e")]

// The suite's areas, one module each, all in `tests/vk_e2e/`. The root is
// `tests/vk_e2e/main.rs`, so Cargo compiles the directory as one test binary
// named `vk_e2e` and every `mod` here resolves beside the root — no `#[path]`
// needed. (The alternative, a top-level `tests/vk_e2e.rs` root, has to name
// each file with `#[path]`, because a crate root's modules otherwise resolve
// beside the root file, which is `tests/` itself.)
mod button_skin;
mod compute;
mod cull;
mod depth_probe;
mod device_request;
mod draw_gen;
mod frame_loop_sequences;
mod harness;
mod indirect;
mod menu;
mod mesh;
mod mesh_shader;
mod nine_slice;
mod pipeline;
mod queries;
mod recording;
mod retire;
mod seam_obligations;
mod sprite;
mod swapchain;
mod triangle;
mod validation_gate;
