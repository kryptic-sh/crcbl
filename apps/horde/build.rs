//! Bakes `assets/*.crpix` into the PNG + Aseprite-sidecar pair the engine
//! reads, and generates the table `src/art.rs` includes.
//!
//! ```text
//! assets/actors.crpix ──parse──▶ CrpixArt ──bake──▶ $OUT_DIR/actors.png
//!                                               └─▶ $OUT_DIR/actors.json
//!                                     │
//!                                     └──▶ $OUT_DIR/art_data.rs (include_bytes!)
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
//! A build script is compiled and run for the *host* whatever the target is, so
//! a `wasm32-unknown-unknown` build of this crate gets the same bytes, already
//! baked, through `include_bytes!`. Nothing reads a file at runtime on any
//! target.

use std::path::PathBuf;

/// The tick rate the frame holds are baked against.
///
/// A `.crpix` counts holds in **simulation ticks** and an Aseprite sidecar
/// counts milliseconds, so the pair has to be converted on the way out and back
/// on the way in — and the two conversions must use the same rate or every hold
/// changes. `bake_dir` writes this number into the generated table as
/// `ART_TICK_HZ`, so the loader reads it rather than declaring its own.
///
/// **One thing horde draws is animated**: the player's walk cycle, in
/// `assets/actors.crpix`. Everything else is a still frame at a moving position,
/// and nothing in this game turns.
///
/// That one clip is what makes the guard on this number worth anything.
/// `art::tests::the_walk_cycle_survives_the_round_trip_through_the_sidecar`
/// asserts each of the walk's frames came back holding for the several ticks
/// that file authored, and every other frame for the default one — where a suite
/// with nothing but default holds can only assert the one, which survives a wide
/// range of wrong arithmetic. Breakout's, asteroids' and `crcbl-render`'s are
/// still in that state; `docs/backlog.md` records it.
const ART_TICK_HZ: u32 = 60;

/// The sheets, by file stem. Each is `assets/<stem>.crpix`.
///
/// **Three, and every split is a batching decision rather than an artistic
/// one.** `assets/actors.crpix` carries the player, all three enemy kinds and
/// the XP pickup in one 34-texel frame size, so the whole field is a single
/// `SpriteRenderer` batch whatever order it is emitted in; the shot is 8 texels
/// and gets its own sheet rather than a twenty-times-oversized quad; the ground
/// is 40 and is a different subject entirely, drawn under everything on its own
/// layer. Each of the three is one batch, and none of the three counts moves
/// with the size of the horde — which is the property `art.rs` argues for and
/// `SceneStats::batches` reports. All three files carry the argument at length.
const ASSETS: [&str; 3] = ["actors", "bolt", "terrain"];

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
        source_label: "apps/horde/assets",
    });
}
