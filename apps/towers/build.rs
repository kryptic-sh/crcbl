//! Bakes `assets/*.crpix` into the PNG + Aseprite-sidecar pair the engine
//! reads, and generates the table `src/art.rs` includes.
//!
//! ```text
//! assets/icons.crpix ──parse──▶ CrpixArt ──bake──▶ $OUT_DIR/icons.png
//!                                              └─▶ $OUT_DIR/icons.json
//!                                    │
//!                                    └──▶ $OUT_DIR/art_data.rs  (include_bytes!)
//! ```
//!
//! The body is [`crcbl_sprite::bake::bake_dir`], as every sample's is; what is
//! here is which sheets exist. Nothing baked is committed, for the reason
//! `apps/asteroids/build.rs` gives: the `.crpix` is the source, and a PNG
//! beside it would be a second picture a reviewer could read instead of the
//! one that loads.

use std::path::PathBuf;

/// The tick rate the frame holds are baked against. Nothing here is animated —
/// every icon is a still frame — so the rate only has to be the one
/// `src/art.rs` reads back, which `bake_dir` writes into the table as
/// `ART_TICK_HZ` for exactly that.
const ART_TICK_HZ: u32 = 60;

/// The sheets, by file stem. Each is `assets/<stem>.crpix`.
///
/// **One sheet**: every icon is the same size, so they share one frame size and
/// one bake, and `src/art.rs` cuts each frame out by name.
const ASSETS: [&str; 1] = ["icons"];

fn main() {
    crcbl_sprite::bake::bake_dir(&crcbl_sprite::bake::BakeDir {
        manifest_dir: &PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("cargo always sets CARGO_MANIFEST_DIR"),
        ),
        out_dir: &PathBuf::from(std::env::var("OUT_DIR").expect("cargo always sets OUT_DIR")),
        stems: &ASSETS,
        tick_hz: ART_TICK_HZ,
        visibility: crcbl_sprite::bake::Visibility::Crate,
        table_name: "art_data.rs",
        source_label: "apps/towers/assets",
    });
}
