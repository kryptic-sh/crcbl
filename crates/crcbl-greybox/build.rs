//! Bakes `assets/*.crpix` — the 2D greybox primitives — into the PNG + sidecar
//! pair [`crcbl_sprite::load`] reads, and generates the table `src/sprite.rs`
//! includes.
//!
//! ```text
//! assets/<stem>.crpix ──parse──▶ CrpixArt ──bake──▶ $OUT_DIR/<stem>.png
//!                                                └─▶ $OUT_DIR/<stem>.json (only if animated)
//!                                            │
//!                                            └──▶ $OUT_DIR/greybox_art.rs  (include_bytes!)
//! ```
//!
//! **The body of this script is [`crcbl_sprite::bake::bake_dir`]** — the same
//! entry point `crates/crcbl-render/build.rs` and the samples call. What is
//! this crate's is the list of sheets and the one thing the others do not do:
//! it bakes only when the `bake` feature is on, so `--no-default-features`
//! builds the 3D pack alone.

use std::path::PathBuf;

/// The tick rate the frame holds are baked against.
///
/// Every greybox primitive is a single still frame — none animates — so no hold
/// is anything but the default of one tick, and the rate never changes a baked
/// duration. It is declared because a `.crpix` counts holds in ticks and an
/// Aseprite sidecar counts milliseconds, and `bake_dir` writes this number into
/// the generated table as `ART_TICK_HZ` so the loader reads it rather than
/// declaring its own.
const ART_TICK_HZ: u32 = 60;

/// The sheets, by file stem. Each is `assets/<stem>.crpix`.
///
/// The 2D prototyping set: tiles and their markers, the round and sloped
/// primitives, the actor placeholders, and the small props. Sizes are in
/// `src/sprite.rs`, which names the texel dimensions the tests assert.
const ASSETS: [&str; 14] = [
    "grid_tile",
    "solid",
    "circle",
    "checker",
    "platform",
    "platform_oneway",
    "slope_45",
    "slope_2to1",
    "ladder",
    "spike",
    "player",
    "enemy",
    "pickup",
    "arrow",
];

fn main() {
    // Nothing to bake for a 3D-only build: `src/sprite.rs` is compiled out with
    // the same feature, so the table it would include is never asked for. The
    // build-dependency's encoder is still linked — cargo compiles build
    // dependencies unconditionally — but it is not run.
    if std::env::var_os("CARGO_FEATURE_BAKE").is_none() {
        return;
    }

    crcbl_sprite::bake::bake_dir(&crcbl_sprite::bake::BakeDir {
        manifest_dir: &PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("cargo always sets CARGO_MANIFEST_DIR"),
        ),
        out_dir: &PathBuf::from(std::env::var("OUT_DIR").expect("cargo always sets OUT_DIR")),
        stems: &ASSETS,
        tick_hz: ART_TICK_HZ,
        visibility: crcbl_sprite::bake::Visibility::Crate,
        table_name: "greybox_art.rs",
        source_label: "crates/crcbl-greybox/assets",
    });
}
