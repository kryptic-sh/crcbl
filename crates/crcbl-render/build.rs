//! Bakes `assets/*.crpix` — the menu's shared art — into the PNG + sidecar pair
//! [`crcbl_sprite::load`] reads, and generates the table `src/menu.rs` includes.
//!
//! ```text
//! assets/menu.crpix ──parse──▶ CrpixArt ──bake──▶ $OUT_DIR/menu.png
//!                                              └─▶ $OUT_DIR/menu.json
//!                                          │
//!                                          └──▶ $OUT_DIR/menu_art.rs  (include_bytes!)
//! ```
//!
//! **The body of this script is [`crcbl_sprite::bake::bake_dir`].** It was
//! written out five times — the four samples and `crates/crcbl-render` — and
//! the copies differed only in their `ASSETS` array, the visibility of the
//! statics they emitted and the name of the file they wrote. Those three are
//! parameters now. What is left here is the part that is genuinely this
//! crate's: which sheets exist, and why they are split the way they are.
//!
//! # Why a renderer crate has art in it
//!
//! Because the menu is the one picture **every sample draws and no sample
//! owns**. `apps/*` cannot depend on each other, so the alternative is the same
//! window frame authored three times — three times the drawing, and three games
//! whose menus look like three different engines. `src/menu.rs` carries the
//! argument in full, including the option that was rejected (a shared `assets/`
//! directory both build scripts reach into).
//!
//! # This is `apps/*/build.rs` for the third time, and that is a finding
//!
//! `apps/breakout/build.rs` and `apps/flappy/build.rs` are already the same
//! script with a different `ASSETS` array, and `docs/backlog.md` has carried the
//! duplication since they were written. This is another copy and it does not
//! close it — the fix is a real `crcbl_sprite::bake::bake_dir` entry point, and
//! it is a change to `crcbl-sprite` rather than to any of its callers, which are
//! every `build.rs` carrying an `ASSETS` array.

use std::path::PathBuf;

/// The tick rate the frame holds are baked against.
///
/// Nothing in the menu animates — the three button frames are states, not a
/// clip — so no hold here is anything but the default of one tick. It is
/// declared because a `.crpix` counts holds in ticks and an Aseprite sidecar
/// counts milliseconds, and the two conversions must use the same rate or every
/// hold silently changes. `bake_dir` writes this number into the generated
/// table as `ART_TICK_HZ`, so the loader reads it rather than declaring its own.
const ART_TICK_HZ: u32 = 60;

/// The sheets, by file stem. Each is `assets/<stem>.crpix`.
///
/// One, and the array stays because the next shared sheet should not have to
/// reintroduce the loop.
const ASSETS: [&str; 1] = ["menu"];

fn main() {
    crcbl_sprite::bake::bake_dir(&crcbl_sprite::bake::BakeDir {
        manifest_dir: &PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("cargo always sets CARGO_MANIFEST_DIR"),
        ),
        out_dir: &PathBuf::from(std::env::var("OUT_DIR").expect("cargo always sets OUT_DIR")),
        stems: &ASSETS,
        tick_hz: ART_TICK_HZ,
        visibility: crcbl_sprite::bake::Visibility::Crate,
        table_name: "menu_art.rs",
        source_label: "crates/crcbl-render/assets",
    });
}
