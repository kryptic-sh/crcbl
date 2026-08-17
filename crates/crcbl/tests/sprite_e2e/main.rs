//! The sprite pass and the two generators in front of it — nine-slice and the
//! shared menu — drawn on whichever backend the registry opens, against
//! checked-in goldens.
//!
//! ```text
//! CRCBL_GPU=vk crates/crcbl/tests/run-sprite-e2e.sh [extra nextest args…]
//! ```
//!
//! # Why this exists
//!
//! Everything about sprites below this line is a recorder assertion or float
//! arithmetic: `crcbl-sprite` and `crcbl_render::sprite_pass` have unit tests for
//! instance bytes, batching and draw ranges, `crcbl_render::nine_slice` asserts
//! its quads to the float, and `crcbl_ui::menu` asserts its layout to the pixel.
//! **None of them can say a pixel landed anywhere.** These tests are the only
//! ones that draw the pass and read the frame back:
//!
//! * [`sprite`] is the pass itself — placement, frame selection, alpha
//!   compositing, sharp-bilinear against a plain-linear control, the pixel snap,
//!   rotation about a quad's own centre, and a mirrored `u` range.
//! * [`nine_slice`] is the stretched slice: corners that kept their size and no
//!   seam between any two bands.
//! * [`button_skin`] is the same claim for a three-state button skin at two
//!   widths a factor of 4.7 apart.
//! * [`menu`] is the shipped art — `crates/crcbl-render/assets/menu.crpix` — in
//!   two framebuffers of different *shapes*, with corner blocks compared between
//!   them.
//!
//! They lived in `crates/crcbl-vk/tests/vk_e2e/` and therefore ran on Vulkan
//! alone, so Metal, D3D12 and wgpu had no evidence that the sprite pass drew
//! anything at all. `tests/render_e2e.rs`'s `sprite` scene is a black-box golden
//! of the *screenshot* path and makes none of the claims above.
//!
//! Nothing here names a backend type — only `crcbl::hal`, `crcbl::render`,
//! `crcbl::math` and `crcbl::backend::open` — so one binary run four times is the
//! whole matrix, exactly as `tests/hal_seam_e2e.rs`, `tests/render_e2e.rs`,
//! `tests/draw_gen_e2e/` and `tests/forward_e2e/` are.
//!
//! # The goldens travel with the tests
//!
//! Nine references moved from `crates/crcbl-vk/tests/golden/` to
//! `crates/crcbl/tests/golden/`, and they are compared through `crcbl-golden` at
//! [`Tolerance::RASTERISER`](crcbl_golden::Tolerance::RASTERISER) — the bound
//! `tests/render_e2e.rs` already holds four backends to — rather than by byte
//! equality. `sprite.png` was renamed `sprite_frames.png` on the way: this
//! crate's golden directory already had a `sprite.png`, which is
//! `render_e2e`'s screenshot scene and a different picture.
//!
//! **The pixel assertions are not a formality beside them.** A golden's tolerance
//! is a fraction of a whole frame, so a one-texel line recoloured inside a large
//! canvas compares equal — `menu`'s bevel check records that being measured
//! rather than assumed. Every module here asserts named pixels first and reports
//! the golden second.
//!
//! # A suite of its own rather than more of `forward_e2e`
//!
//! It shares [`harness`] and nothing else, which is the same relationship
//! `forward_e2e` and `draw_gen_e2e` have with each other. This one draws through
//! [`SpriteRenderer`](crcbl::render::SpriteRenderer) and
//! [`MenuRenderer`](crcbl::render::MenuRenderer) at a pinned `Rgba8UnormSrgb`,
//! commits images, and never builds a `ForwardRenderer` or a scene; a machine
//! that can run one should not have to run the other to get an answer.
//!
//! # The backend must be named
//!
//! Every backend is meant to produce these pixels identically, so a run that
//! silently fell back to another backend passes and proves nothing about the one
//! that was wanted. [`harness`]'s `instance` compares the opened backend against
//! `CRCBL_GPU`, and `tests/run-sprite-e2e.sh` refuses to run without it.

#![cfg(feature = "sprite-e2e")]

/// What this binary calls itself in every line it prints and every debug label
/// it sets.
///
/// Read by `tests/gpu_scene/harness.rs`, which is shared with
/// `tests/draw_gen_e2e/` and `tests/forward_e2e/` and therefore cannot name any
/// of them. **This string is `tests/run-sprite-e2e.sh`'s grep**: the runner finds
/// the adapter line by its prefix, so changing it turns a green suite into a
/// failed harness run.
pub(crate) const SUITE: &str = "crcbl sprite e2e";

// The suite's areas, one module each, all in `tests/sprite_e2e/`. The root is
// `tests/sprite_e2e/main.rs`, so Cargo compiles the directory as one test binary
// named `sprite_e2e` and every `mod` here resolves beside the root.
//
// `sprite` owns the fixture the other three import — the extent the goldens were
// blessed at, the clear colour, the camera and its `world_to_pixel` mapping, the
// test sheets and the golden helper — because a button skin, a nine-slice and a
// menu are all sprite quads with a different generator in front of them.
mod button_skin;
mod menu;
mod nine_slice;
mod sprite;

// The fixture, out of `tests/gpu_scene/` rather than beside the root, because
// `tests/draw_gen_e2e/` and `tests/forward_e2e/` open the same device against the
// same offscreen ring and a second copy is a second place a fix has to land.
// That directory holds no `main.rs`, so Cargo builds no target of its own from
// it.
#[path = "../gpu_scene/harness.rs"]
mod harness;
