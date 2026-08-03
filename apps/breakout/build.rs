//! Bakes `assets/*.crpix` into the PNG + Aseprite-sidecar pair the engine
//! reads, and generates the table `src/art.rs` includes.
//!
//! ```text
//! assets/bricks.crpix ──parse──▶ CrpixArt ──bake──▶ $OUT_DIR/bricks.png
//!                                               └─▶ $OUT_DIR/bricks.json
//!                                     │
//!                                     └──▶ $OUT_DIR/art_data.rs  (include_bytes!)
//! ```
//!
//! **The body of this script is [`crcbl_sprite::bake::bake_dir`].** It was
//! written out five times — the four samples and `crates/crcbl-render` — and
//! the copies differed only in their `ASSETS` array, the visibility of the
//! statics they emitted and the name of the file they wrote. Those three are
//! parameters now. What is left here is the part that is genuinely this
//! crate's: which sheets exist, and why they are split the way they are.
//!
//! # Why the baked files are not committed
//!
//! `docs/specs/crcbl/pix.md` is explicit that `.crpix` is a **build input** and
//! that nothing downstream knows it exists. Committing the PNG beside the text
//! would create two sources of truth for the same picture, and the one a
//! reviewer reads in a diff would be the one that is not loaded.
//!
//! # This runs on the host, for every target
//!
//! `apps/breakout` builds for `wasm32-unknown-unknown`, and a build script is
//! compiled and run for the *host* whatever the target is. The browser build
//! gets the same bytes, already baked, through `include_bytes!`, and nothing
//! reads a file at runtime on any target.

use std::path::PathBuf;

/// The tick rate the frame holds are baked against.
///
/// A `.crpix` counts holds in **simulation ticks** and an Aseprite sidecar
/// counts milliseconds, so the pair has to be converted on the way out and back
/// on the way in — and the two conversions must use the same rate or every hold
/// changes. `bake_dir` writes this number into the generated table as
/// `ART_TICK_HZ`, so the loader reads it rather than declaring its own.
///
/// Nothing breakout draws is animated today, so no hold in this game's art is
/// longer than the default of one tick; the rate is here so that the first
/// animated frame does not arrive alongside a second, silent convention.
const ART_TICK_HZ: u32 = 60;

/// The sheets, by file stem. Each is `assets/<stem>.crpix`.
const ASSETS: [&str; 4] = ["bricks", "paddle", "ball", "field"];

fn main() {
    crcbl_sprite::bake::bake_dir(&crcbl_sprite::bake::BakeDir {
        manifest_dir: &PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("cargo always sets CARGO_MANIFEST_DIR"),
        ),
        out_dir: &PathBuf::from(std::env::var("OUT_DIR").expect("cargo always sets OUT_DIR")),
        stems: &ASSETS,
        tick_hz: ART_TICK_HZ,
        visibility: crcbl_sprite::bake::Visibility::Public,
        table_name: "art_data.rs",
        source_label: "apps/breakout/assets",
    });
}
