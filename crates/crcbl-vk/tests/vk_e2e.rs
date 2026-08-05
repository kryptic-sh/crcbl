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

// The suite's areas, one module each, all in `tests/vk_e2e/` — because Cargo
// compiles every top-level `tests/*.rs` as its own test binary, and these are
// parts of *this* one. `#[path]` is what puts them there: a crate root's
// modules otherwise resolve beside the root file, which is `tests/` itself.
#[path = "vk_e2e/button_skin.rs"]
mod button_skin;
#[path = "vk_e2e/compute.rs"]
mod compute;
#[path = "vk_e2e/depth_probe.rs"]
mod depth_probe;
#[path = "vk_e2e/device_request.rs"]
mod device_request;
#[path = "vk_e2e/frame_loop_sequences.rs"]
mod frame_loop_sequences;
#[path = "vk_e2e/harness.rs"]
mod harness;
#[path = "vk_e2e/indirect.rs"]
mod indirect;
#[path = "vk_e2e/menu.rs"]
mod menu;
#[path = "vk_e2e/mesh.rs"]
mod mesh;
#[path = "vk_e2e/nine_slice.rs"]
mod nine_slice;
#[path = "vk_e2e/pipeline.rs"]
mod pipeline;
#[path = "vk_e2e/queries.rs"]
mod queries;
#[path = "vk_e2e/recording.rs"]
mod recording;
#[path = "vk_e2e/retire.rs"]
mod retire;
#[path = "vk_e2e/seam_obligations.rs"]
mod seam_obligations;
#[path = "vk_e2e/sprite/mod.rs"]
mod sprite;
#[path = "vk_e2e/swapchain.rs"]
mod swapchain;
#[path = "vk_e2e/triangle.rs"]
mod triangle;
#[path = "vk_e2e/validation_gate.rs"]
mod validation_gate;
